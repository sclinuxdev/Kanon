"""Plugin base class and decorators for Kanon Python SDK."""

import inspect
from typing import Any, Callable, Dict, List, Optional
from google.protobuf.json_format import MessageToDict, ParseDict
from google.protobuf.struct_pb2 import Struct

from kanon_sdk.context import PluginContext
from kanon_sdk.proto import pb


def command(
    name: str,
    description: str = "",
    usage: str = "",
    priority: int = 500,
) -> Callable:
    """Decorator to declare a command handler method within a Plugin class."""
    def decorator(fn: Callable) -> Callable:
        fn._kanon_command = {
            "name": name,
            "description": description,
            "usage": usage,
            "priority": priority,
        }
        return fn
    return decorator


def tool(
    name: str,
    description: str = "",
    parameters: Optional[Dict[str, Any]] = None,
) -> Callable:
    """Decorator to declare an LLM tool call handler method within a Plugin class."""
    def decorator(fn: Callable) -> Callable:
        fn._kanon_tool = {
            "name": name,
            "description": description,
            "parameters": parameters or {},
        }
        return fn
    return decorator


class Plugin:
    """Abstract base class for Kanon out-of-process Python plugins."""

    id: str = "org.kanon.plugin.base"
    name: str = "Base Python Plugin"
    version: str = "0.1.0"
    author: str = "Kanon Dev"
    description: str = "Default Python plugin"
    priority: int = 500

    def __init__(self) -> None:
        self.context: Optional[PluginContext] = None
        self._command_handlers: Dict[str, Callable] = {}
        self._tool_handlers: Dict[str, Callable] = {}
        self._collect_decorated_handlers()

    def _collect_decorated_handlers(self) -> None:
        """Inspects methods on self to discover @command and @tool annotations."""
        for attr_name in dir(self):
            try:
                attr = getattr(self, attr_name)
            except Exception:
                continue

            if callable(attr):
                if hasattr(attr, "_kanon_command"):
                    cmd_meta = getattr(attr, "_kanon_command")
                    self._command_handlers[cmd_meta["name"]] = attr

                if hasattr(attr, "_kanon_tool"):
                    tool_meta = getattr(attr, "_kanon_tool")
                    self._tool_handlers[tool_meta["name"]] = attr

    def meta(self) -> pb.PluginMeta:
        """Constructs and returns static metadata for this plugin."""
        commands: List[pb.CommandMeta] = []
        for cmd_name, handler in self._command_handlers.items():
            info = getattr(handler, "_kanon_command")
            commands.append(
                pb.CommandMeta(
                    name=info["name"],
                    description=info["description"],
                    usage=info["usage"],
                    priority=info["priority"],
                )
            )

        tools: List[pb.ToolMeta] = []
        for tool_name, handler in self._tool_handlers.items():
            info = getattr(handler, "_kanon_tool")
            param_struct = Struct()
            if info["parameters"]:
                ParseDict(info["parameters"], param_struct)
            tools.append(
                pb.ToolMeta(
                    name=info["name"],
                    description=info["description"],
                    parameters=param_struct if info["parameters"] else None,
                )
            )

        return pb.PluginMeta(
            id=self.id,
            name=self.name,
            version=self.version,
            author=self.author,
            description=self.description,
            commands=commands,
            tools=tools,
        )

    async def on_load(self, ctx: PluginContext) -> None:
        """Lifecycle hook invoked when the plugin host finishes loading the plugin."""
        self.context = ctx

    async def on_unload(self) -> None:
        """Lifecycle hook invoked prior to plugin shutdown and process termination."""
        pass

    async def on_pre_filter(
        self,
        req: pb.PipelineEventRequest,
    ) -> Optional[pb.PreFilterResult]:
        """Intercepts inbound messages before command and LLM dispatching.

        Returning None or PreFilterResult with PASS allows downstream execution.
        Returning PreFilterResult with BLOCK halts downstream processing.
        """
        return None

    async def on_execute_command(
        self,
        req: pb.CommandExecuteRequest,
    ) -> pb.CommandExecuteResponse:
        """Executes a matched slash command.

        Dispatches to an annotated @command handler if present, or returns an error.
        """
        handler = self._command_handlers.get(req.command)
        if handler:
            # Inspect signature to see if args/context are expected
            sig = inspect.signature(handler)
            params = list(sig.parameters.values())

            if len(params) == 0:
                result = await handler() if inspect.iscoroutinefunction(handler) else handler()
            elif len(params) == 1:
                result = await handler(req) if inspect.iscoroutinefunction(handler) else handler(req)
            else:
                result = await handler(req, req.args) if inspect.iscoroutinefunction(handler) else handler(req, req.args)

            if isinstance(result, pb.CommandExecuteResponse):
                return result
            elif isinstance(result, list):
                return pb.CommandExecuteResponse(success=True, replies=result)
            elif isinstance(result, str):
                from kanon_sdk.context import MessageSegment
                return pb.CommandExecuteResponse(
                    success=True,
                    replies=[MessageSegment.text(result)],
                )
            return pb.CommandExecuteResponse(success=True)

        return pb.CommandExecuteResponse(
            success=False,
            error_message=f"Unknown command: {req.command}",
        )

    async def on_call_tool(
        self,
        req: pb.ToolCallRequest,
    ) -> pb.ToolCallResponse:
        """Executes an LLM tool call dispatched by the Core microkernel."""
        handler = self._tool_handlers.get(req.tool_name)
        if handler:
            args_dict: Dict[str, Any] = {}
            if req.HasField("structured_args"):
                args_dict = MessageToDict(req.structured_args)

            result = await handler(args_dict) if inspect.iscoroutinefunction(handler) else handler(args_dict)

            if isinstance(result, pb.ToolCallResponse):
                return result
            elif isinstance(result, bytes):
                return pb.ToolCallResponse(
                    call_id=req.call_id,
                    success=True,
                    raw_bytes=result,
                )
            elif isinstance(result, dict):
                result_struct = Struct()
                ParseDict(result, result_struct)
                return pb.ToolCallResponse(
                    call_id=req.call_id,
                    success=True,
                    structured_result=result_struct,
                )
            else:
                result_struct = Struct()
                ParseDict({"result": str(result)}, result_struct)
                return pb.ToolCallResponse(
                    call_id=req.call_id,
                    success=True,
                    structured_result=result_struct,
                )

        return pb.ToolCallResponse(
            call_id=req.call_id,
            success=False,
            error_message=f"Unknown tool: {req.tool_name}",
        )

    async def on_event(self, req: pb.EventNotification) -> None:
        """Processes an asynchronous event notification broadcast from Core."""
        pass

    async def on_deliver_message(
        self,
        req: pb.DeliverMessageRequest,
    ) -> pb.DeliverMessageResponse:
        """Delivers an outbound message to a target platform."""
        return pb.DeliverMessageResponse(
            success=True,
            message_id="delivered_py_1",
        )
