"""Core IPC channel construction for the Kanon Python SDK.

Why this module exists
----------------------
Every Python participant in the Kanon topology (plugin host, platform adapter, management
tooling) speaks gRPC to exactly one endpoint: the Core microkernel's Unix domain socket.
Dialing it is *not* a plain ``insecure_channel`` call, because gRPC's C-core derives the
HTTP/2 ``:authority`` pseudo-header from the channel target: for a target written as
``unix:/abs/path/core.sock`` it sends the **percent-encoded socket path** as the authority,
e.g. ``run%2Fuser%2F1000%2Fkanon%2Frun%2Fcore.sock``.

That value is not a legal HTTP/2 authority. Percent escapes are only tolerated inside a
userinfo component or an IPv6 zone identifier, so Rust's ``h2`` server -- the HTTP/2
implementation behind Tonic, which serves ``core.sock`` -- rejects the request headers and
resets the stream before any gRPC status can be produced::

    DEBUG h2::server: malformed headers: malformed authority
          (b"run%2Fuser%2F1000%2Fkanon%2Frun%2Fcore.sock"): invalid authority

The caller only observes the resulting opaque transport failure, which names neither the
authority nor the offending field and therefore looks like a Core crash::

    StatusCode.INTERNAL
    "Stream removed (RST_STREAM (Received RST_STREAM with error code 1))"

*Every* ``BotApiService`` RPC fails this way (``RegisterHost``, ``IngestEvent``,
``SendMessage``, ...): the plugin host silently degrades to standalone mode and the adapter
drops inbound messages, even though Core is perfectly healthy.

The fix is to pin an explicit, valid default authority on the channel, which is exactly what
this module exists to guarantee in one place. Note that rewriting the target as
``unix:///abs/path/core.sock`` does *not* help: C-core still percent-encodes the path into
the authority. Other SDKs are unaffected because they never expose the socket path as an
authority -- gRPC-js hard-codes ``localhost`` for its ``unix:`` resolver and the Rust SDK
dials through Tonic's ``http://localhost`` endpoint.
"""

from __future__ import annotations

import asyncio
import os
import time
from pathlib import Path
from typing import Callable, Optional, Union

import grpc

#: Fallback endpoint used when ``KANON_CORE_SOCK`` is not injected by the Supervisor.
#: The Supervisor always injects the real path, so this only serves manual/standalone runs.
DEFAULT_CORE_SOCK = "./run/core.sock"

#: Default HTTP/2 authority handed to Core for every RPC.
#:
#: Any syntactically valid authority works (Core never reads it: the peer identity is the
#: Unix socket credential, and Windows loopback authentication uses the ``x-kanon-auth-token``
#: metadata header instead). ``localhost`` is chosen to match what gRPC-js and the Rust SDK
#: already send, so all three runtimes present the same request shape to the same endpoint.
CORE_AUTHORITY = "localhost"


def connect_core_channel(core_sock: Union[str, Path]) -> grpc.aio.Channel:
    """Creates the long-lived gRPC channel to Kanon Core over its Unix domain socket.

    The returned channel is lazy: it dials on the first RPC and reconnects on its own after a
    Core restart, so callers own it for the whole process lifetime and must close it once.

    Args:
        core_sock: Path of the Core endpoint (``core.sock``), typically taken from
            ``KANON_CORE_SOCK``.

    Returns:
        A channel whose requests carry a valid HTTP/2 authority. See the module docstring for
        why the authority must never be left to C-core's target-derived default.
    """
    # ``grpc.default_authority`` is the only knob that suppresses C-core's percent-encoded
    # authority: it overrides the pseudo-header verbatim for every RPC on the channel.
    return grpc.aio.insecure_channel(
        f"unix:{core_sock}",
        options=[("grpc.default_authority", CORE_AUTHORITY)],
    )


#: Interval between core liveness probes, in seconds.
CORE_WATCHDOG_INTERVAL = 15.0

#: Per-probe deadline, in seconds.
CORE_WATCHDOG_TIMEOUT = 5.0

#: Consecutive failed probes tolerated before the host stops itself.
CORE_WATCHDOG_FAILURES = 3


class CoreWatchdog:
    """Stops a plugin host once the core it registered with is gone.

    Why this exists
    ---------------
    A host is a child of the core, but ``SIGKILL`` and manual cleanups do not always reap it: the
    process survives, keeps its platform connection (a QQ gateway socket, a Telegram poller) and
    keeps pushing events into a core that no longer exists. When a *new* core starts, the orphan
    and the fresh host both serve the same platform, so every inbound message is handled twice —
    two replies, two model calls, two bills — while nothing looks broken in the console.

    The watchdog probes ``BotApiService.Ping`` on the core. Any answer proves the core is alive;
    when the probe fails :data:`CORE_WATCHDOG_FAILURES` times in a row the host is asked to stop,
    which unloads the plugin and closes its platform connection.
    """

    def __init__(
        self,
        stub: object,
        pb: object,
        *,
        interval: float = CORE_WATCHDOG_INTERVAL,
        timeout: float = CORE_WATCHDOG_TIMEOUT,
        failures: int = CORE_WATCHDOG_FAILURES,
        on_lost: Optional[Callable[[str], None]] = None,
    ) -> None:
        self._stub = stub
        self._pb = pb
        self._interval = interval
        self._timeout = timeout
        self._failures = failures
        self._on_lost = on_lost
        self._task: Optional[asyncio.Task] = None

    def start(self) -> asyncio.Task:
        """Starts the background probe loop."""
        self._task = asyncio.create_task(self._run())
        return self._task

    async def stop(self) -> None:
        """Stops the probe loop, if running."""
        if self._task is None:
            return
        self._task.cancel()
        try:
            await self._task
        except (asyncio.CancelledError, Exception):
            pass
        self._task = None

    async def _run(self) -> None:
        consecutive = 0
        while True:
            await asyncio.sleep(self._interval)
            try:
                request = self._pb.PingRequest(timestamp=int(time.time() * 1000))
                await self._stub.Ping(request, timeout=self._timeout)
                consecutive = 0
            except asyncio.CancelledError:
                raise
            except Exception as exc:  # noqa: BLE001 - any failure means "core not answering"
                consecutive += 1
                print(
                    f"[kanon-host] core liveness probe failed ({consecutive}/{self._failures}): {exc}",
                    flush=True,
                )
                if consecutive >= self._failures:
                    reason = (
                        f"core unreachable for {consecutive} consecutive probes; "
                        "stopping this host so it cannot serve the platform without a core"
                    )
                    print(f"[kanon-host] {reason}", flush=True)
                    if self._on_lost is not None:
                        self._on_lost(reason)
                    return
