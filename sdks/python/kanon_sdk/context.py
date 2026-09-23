"""Context and message segment abstractions for Kanon Python SDK."""

from pathlib import Path
from typing import Any, Dict, Optional

from kanon_sdk.proto import pb


class PluginContext:
    """Runtime context provided to a plugin during initialization and execution."""

    def __init__(self, data_dir: Path, config: Optional[Dict[str, Any]] = None):
        self.data_dir = data_dir
        self.config = config or {}


class MessageSegment:
    """Helper factory for constructing strongly typed Protobuf MessageSegment instances."""

    @staticmethod
    def text(content: str) -> pb.MessageSegment:
        """Constructs a plain text segment."""
        return pb.MessageSegment(text=pb.TextSegment(content=content))

    @staticmethod
    def image_url(
        url: str,
        mime_type: Optional[str] = None,
        filename: Optional[str] = None,
    ) -> pb.MessageSegment:
        """Constructs an image segment referencing a remote URL."""
        return pb.MessageSegment(
            image=pb.ImageSegment(
                url=url,
                mime_type=mime_type,
                filename=filename,
            )
        )

    @staticmethod
    def image_file(
        file_path: str,
        mime_type: Optional[str] = None,
        filename: Optional[str] = None,
    ) -> pb.MessageSegment:
        """Constructs an image segment referencing a local zero-copy physical file."""
        return pb.MessageSegment(
            image=pb.ImageSegment(
                file_path=file_path,
                mime_type=mime_type,
                filename=filename,
            )
        )

    @staticmethod
    def image_bytes(
        raw_bytes: bytes,
        mime_type: Optional[str] = None,
        filename: Optional[str] = None,
    ) -> pb.MessageSegment:
        """Constructs an image segment from raw byte payload."""
        return pb.MessageSegment(
            image=pb.ImageSegment(
                raw_bytes=raw_bytes,
                mime_type=mime_type,
                filename=filename,
            )
        )
