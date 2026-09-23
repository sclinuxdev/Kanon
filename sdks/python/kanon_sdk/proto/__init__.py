"""Protobuf and gRPC definitions for Kanon plugin protocol."""

import os
import sys

# Ensure proto directory is in sys.path for generated pb2 relative imports
_proto_dir = os.path.dirname(__file__)
if _proto_dir not in sys.path:
    sys.path.insert(0, _proto_dir)

from . import plugin_pb2 as pb
from . import plugin_pb2_grpc as pb_grpc

__all__ = ["pb", "pb_grpc"]
