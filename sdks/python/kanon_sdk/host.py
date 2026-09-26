"""Out-of-process gRPC plugin host for Kanon Python SDK."""

import asyncio
import os
import signal
import sys
from pathlib import Path
from typing import Optional, Union

import grpc

from kanon_sdk.context import CoreHandle, PluginContext
from kanon_sdk.plugin import Plugin
from kanon_sdk.proto import pb, pb_grpc


class HostServiceImpl(pb_grpc.PluginHostServiceServicer):
    """Implementation of PluginHostService for host lifecycle management."""

    def __init__(self, plugin: Plugin):
        self.plugin = plugin
        self._config_version = 0

    async def Ping(
        self,
        request: pb.PingRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.PingResponse:
        return pb.PingResponse(timestamp=request.timestamp)

    async def ReloadPluginConfig(
        self,
        request: pb.ReloadPluginConfigRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.ReloadPluginConfigResponse:
        if request.version > 0 and request.version <= self._config_version:
            return pb.ReloadPluginConfigResponse(
                success=False,
                error_message=f"Stale config version {request.version}: current is {self._config_version}",
                applied_version=self._config_version,
            )
        self._config_version = request.version
        if request.HasField("config"):
            from google.protobuf.json_format import MessageToDict
            new_config = MessageToDict(request.config)
            if self.plugin.context is not None:
                self.plugin.context.config = new_config
            try:
                await self.plugin.on_config_reload(new_config)
            except Exception as exc:
                return pb.ReloadPluginConfigResponse(
                    success=False,
                    error_message=f"on_config_reload failed: {exc}",
                    applied_version=self._config_version,
                )
        return pb.ReloadPluginConfigResponse(
            success=True,
            error_message="",
            applied_version=request.version,
        )

    async def GetPluginMeta(
        self,
        request: pb.GetPluginMetaRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.GetPluginMetaResponse:
        meta = self.plugin.meta()
        return pb.GetPluginMetaResponse(plugins=[meta])


class PipelineServiceImpl(pb_grpc.MessagePipelineServiceServicer):
    """Implementation of MessagePipelineService for dispatching pipeline events."""

    def __init__(self, plugin: Plugin):
        self.plugin = plugin

    async def OnPreFilter(
        self,
        request: pb.PipelineEventRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.PreFilterResult:
        result = await self.plugin.on_pre_filter(request)
        if result is None:
            return pb.PreFilterResult(
                action=pb.PreFilterResult.Action.PASS,
                modified_text="",
                reply_messages=[],
            )
        return result

    async def OnExecuteCommand(
        self,
        request: pb.CommandExecuteRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.CommandExecuteResponse:
        return await self.plugin.on_execute_command(request)

    async def OnCallTool(
        self,
        request: pb.ToolCallRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.ToolCallResponse:
        return await self.plugin.on_call_tool(request)

    async def OnEvent(
        self,
        request: pb.EventNotification,
        context: grpc.aio.ServicerContext,
    ) -> pb.EventAck:
        await self.plugin.on_event(request)
        return pb.EventAck(received=True)

    async def OnDeliverMessage(
        self,
        request: pb.DeliverMessageRequest,
        context: grpc.aio.ServicerContext,
    ) -> pb.DeliverMessageResponse:
        return await self.plugin.on_deliver_message(request)


class KanonHost:
    """Out-of-process gRPC host for running a Kanon Python plugin."""

    def __init__(
        self,
        plugin: Plugin,
        socket_path: Optional[Union[str, Path]] = None,
        core_sock: Optional[Union[str, Path]] = None,
        host_id: Optional[str] = None,
        data_dir: Optional[Union[str, Path]] = None,
    ):
        self.plugin = plugin
        self.socket_path = (
            Path(socket_path).resolve()
            if socket_path
            else Path(os.environ.get("KANON_HOST_SOCK", "./run/host_py.sock")).resolve()
        )
        self.core_sock = (
            Path(core_sock).resolve()
            if core_sock
            else (
                Path(os.environ["KANON_CORE_SOCK"]).resolve()
                if os.environ.get("KANON_CORE_SOCK")
                else (Path("./run/core.sock").resolve() if Path("./run/core.sock").exists() else None)
            )
        )
        self.host_id = host_id or os.environ.get("KANON_HOST_ID", f"host_{plugin.id.replace('.', '_')}")
        self.data_dir = (
            Path(data_dir).resolve()
            if data_dir
            else Path(f"./data/plugins/{plugin.id}").resolve()
        )

    def run(self) -> None:
        """Runs the plugin host synchronously until interrupted."""
        asyncio.run(self.run_async())

    async def run_async(self, shutdown_event: Optional[asyncio.Event] = None) -> None:
        """Runs the plugin host asynchronously until shutdown is requested."""
        meta = self.plugin.meta()
        self.data_dir.mkdir(parents=True, exist_ok=True)

        # Clean up stale socket file if it exists
        if self.socket_path.exists():
            try:
                self.socket_path.unlink()
            except OSError:
                pass

        self.socket_path.parent.mkdir(parents=True, exist_ok=True)

        # Start host gRPC server first so that Core can reach it during RegisterHost
        server = grpc.aio.server()
        pb_grpc.add_PluginHostServiceServicer_to_server(HostServiceImpl(self.plugin), server)
        pb_grpc.add_MessagePipelineServiceServicer_to_server(PipelineServiceImpl(self.plugin), server)

        server.add_insecure_port(f"unix:{self.socket_path}")
        await server.start()
        print(f"Kanon Python Host running on {self.socket_path}", flush=True)

        core_channel: Optional[grpc.aio.Channel] = None
        core_handle: Optional[CoreHandle] = None

        if self.core_sock:
            if self.core_sock.exists():
                try:
                    core_channel = grpc.aio.insecure_channel(f"unix:{self.core_sock}")
                    core_stub = pb_grpc.BotApiServiceStub(core_channel)
                    reg_req = pb.RegisterHostRequest(
                        host_id=self.host_id,
                        runtime="python",
                        endpoint=str(self.socket_path),
                        loaded_plugin_ids=[meta.id],
                    )
                    await core_stub.RegisterHost(reg_req, timeout=3.0)
                    core_handle = CoreHandle(core_stub)
                except Exception as exc:
                    if core_channel is not None:
                        try:
                            await core_channel.close()
                        except Exception:
                            pass
                    core_channel = None
                    print(
                        f"[kanon-host] Core at {self.core_sock} unreachable ({exc}); "
                        "running in standalone mode with ctx.core = None",
                        file=sys.stderr,
                        flush=True,
                    )
            else:
                print(
                    f"[kanon-host] KANON_CORE_SOCK points to missing socket {self.core_sock}; "
                    "running in standalone mode with ctx.core = None",
                    file=sys.stderr,
                    flush=True,
                )
        else:
            print(
                "[kanon-host] KANON_CORE_SOCK is not set; "
                "running in standalone mode with ctx.core = None",
                file=sys.stderr,
                flush=True,
            )

        config: dict = {}
        config_path = self.data_dir / "config.json"
        if config_path.exists():
            try:
                import json
                config = json.loads(config_path.read_text(encoding="utf-8"))
            except Exception as e:
                print(f"[kanon-host] Failed to load config from {config_path}: {e}", file=sys.stderr, flush=True)

        ctx = PluginContext(data_dir=self.data_dir, config=config, core=core_handle)
        await self.plugin.on_load(ctx)

        if shutdown_event is None:
            stop_event = asyncio.Event()

            def handle_signal():
                stop_event.set()

            loop = asyncio.get_running_loop()
            for sig in (signal.SIGINT, signal.SIGTERM):
                try:
                    loop.add_signal_handler(sig, handle_signal)
                except (NotImplementedError, RuntimeError):
                    pass

            await stop_event.wait()
        else:
            await shutdown_event.wait()

        # Graceful shutdown
        await self.plugin.on_unload()
        await server.stop(grace=1.0)
        if core_channel is not None:
            await core_channel.close()
        if self.socket_path.exists():
            try:
                self.socket_path.unlink()
            except OSError:
                pass
