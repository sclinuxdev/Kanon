"""Official Kanon Python SDK.

Provides base classes, decorators, and context abstractions for authoring
out-of-process Kanon plugins in Python.
"""

from kanon_sdk.context import CoreHandle, MessageSegment, PluginContext
from kanon_sdk.host import KanonHost
from kanon_sdk.ipc import connect_core_channel
from kanon_sdk.plugin import Plugin, action, command, tool
from kanon_sdk.proto import pb, pb_grpc

__all__ = [
    "CoreHandle",
    "KanonHost",
    "MessageSegment",
    "PluginContext",
    "Plugin",
    "action",
    "command",
    "connect_core_channel",
    "tool",
    "pb",
    "pb_grpc",
]
