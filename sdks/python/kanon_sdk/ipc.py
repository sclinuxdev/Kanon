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

from pathlib import Path
from typing import Union

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
