"""Outbound message delivery manager for Kanon QQ Official platform adapter.

Handles message segmentation (text, images, audio), Markdown formatting,
automatic plain text fallback on permission errors, and large asset uploads.
"""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import TYPE_CHECKING, Any, Dict, List, Optional

from botpy.types.message import MarkdownPayload, Media
from kanon_sdk.proto import pb

from upload import (
    QQOfficialChunkedUploader,
)

if TYPE_CHECKING:
    from client import KanonBotClient
    from main import QQOfficialAdapter


class QQDeliveryManager:
    """Manages outbound delivery of messages and media to QQ platform."""

    def __init__(self, adapter: QQOfficialAdapter) -> None:
        self.adapter = adapter
        self._msg_seq_cache: Dict[str, int] = {}

    @property
    def bot_client(self) -> Optional[KanonBotClient]:
        """Returns the active bot client instance."""
        return self.adapter.bot_client

    @property
    def uploader(self) -> Optional[QQOfficialChunkedUploader]:
        """Returns the active chunked uploader instance."""
        return self.adapter.uploader

    def _next_msg_seq(self, msg_id: Optional[str]) -> int:
        """Increments and returns message sequence number for passive replies."""
        if not msg_id:
            return 1
        seq = self._msg_seq_cache.get(msg_id, 0) + 1
        self._msg_seq_cache[msg_id] = seq
        if len(self._msg_seq_cache) > 2000:
            for k in list(self._msg_seq_cache.keys())[:500]:
                self._msg_seq_cache.pop(k, None)
        return seq

    async def safe_post_group_message(self, **kwargs: Any) -> Any:
        """Sends group message with automatic fallback to active sending on passive error."""
        if not self.bot_client or self.bot_client.is_closed():
            raise RuntimeError("QQ client is not running or disconnected")

        msg_id = kwargs.get("msg_id")
        seq = self._next_msg_seq(msg_id)
        kwargs["msg_seq"] = seq
        try:
            return await self.bot_client.api.post_group_message(**kwargs)
        except Exception as e:
            err = str(e).lower()
            if msg_id and ("msg_id" in err or "越权" in err or "304023" in err or "10004" in err):
                print(
                    f"[QQOfficial] Passive msg_id '{msg_id}' rejected ({e}); retrying as active message",
                    flush=True,
                )
                retry_kwargs = dict(kwargs)
                retry_kwargs["msg_id"] = None
                return await self.bot_client.api.post_group_message(**retry_kwargs)
            raise

    async def safe_post_c2c_message(self, **kwargs: Any) -> Any:
        """Sends C2C message with automatic fallback to active sending on passive error."""
        if not self.bot_client or self.bot_client.is_closed():
            raise RuntimeError("QQ client is not running or disconnected")

        msg_id = kwargs.get("msg_id")
        seq = self._next_msg_seq(msg_id)
        kwargs["msg_seq"] = seq
        try:
            return await self.bot_client.api.post_c2c_message(**kwargs)
        except Exception as e:
            err = str(e).lower()
            if msg_id and ("msg_id" in err or "越权" in err or "304023" in err or "10004" in err):
                print(
                    f"[QQOfficial] Passive msg_id '{msg_id}' rejected ({e}); retrying as active message",
                    flush=True,
                )
                retry_kwargs = dict(kwargs)
                retry_kwargs["msg_id"] = None
                return await self.bot_client.api.post_c2c_message(**retry_kwargs)
            raise

    async def deliver_message(
        self,
        req: pb.DeliverMessageRequest,
    ) -> pb.DeliverMessageResponse:
        """Delivers outbound messages back to QQ platform with rich media & markdown fallback."""
        if not self.bot_client or self.bot_client.is_closed():
            return pb.DeliverMessageResponse(
                success=False,
                error_message="QQ client is not running or disconnected",
            )

        text_parts: List[str] = []
        image_sources: List[str] = []
        audio_sources: List[str] = []

        for segment in req.segments:
            if segment.HasField("text"):
                text_parts.append(segment.text.content)
            elif segment.HasField("image"):
                src = segment.image.url or segment.image.file_path
                if src:
                    image_sources.append(src)
            elif segment.HasField("audio"):
                src = segment.audio.url or segment.audio.file_path
                if src:
                    audio_sources.append(src)

        plain_text = "\n".join(text_parts).strip()
        config = self.adapter.context.config if self.adapter.context else {}
        use_markdown = bool(config.get("use_markdown", True))
        template_id = str(config.get("markdown_template_id", "") or "")
        param_key = str(config.get("markdown_params_key", "text_content") or "text_content")

        channel_id = req.channel_id
        msg_id = req.event_id or None

        try:
            if channel_id.startswith("group:"):
                group_openid = channel_id.removeprefix("group:")
                await self._deliver_group(
                    group_openid=group_openid,
                    plain_text=plain_text,
                    image_sources=image_sources,
                    use_markdown=use_markdown,
                    template_id=template_id,
                    param_key=param_key,
                    msg_id=msg_id,
                )
            elif channel_id.startswith("c2c:"):
                user_openid = channel_id.removeprefix("c2c:")
                await self._deliver_c2c(
                    user_openid=user_openid,
                    plain_text=plain_text,
                    image_sources=image_sources,
                    use_markdown=use_markdown,
                    template_id=template_id,
                    param_key=param_key,
                    msg_id=msg_id,
                )
            elif channel_id.startswith("guild:"):
                guild_channel_id = channel_id.removeprefix("guild:")
                await self._deliver_guild(
                    channel_id=guild_channel_id,
                    plain_text=plain_text,
                    image_sources=image_sources,
                    msg_id=msg_id,
                )
            elif channel_id.startswith("guild_dm:"):
                dm_channel_id = channel_id.removeprefix("guild_dm:")
                await self._deliver_guild_dm(
                    guild_id=dm_channel_id,
                    plain_text=plain_text,
                    msg_id=msg_id,
                )
            else:
                return pb.DeliverMessageResponse(
                    success=False,
                    error_message=f"Unsupported channel scheme in: '{channel_id}'",
                )

            return pb.DeliverMessageResponse(success=True, message_id=req.event_id)
        except Exception as e:
            return pb.DeliverMessageResponse(success=False, error_message=str(e))

    async def _deliver_group(
        self,
        group_openid: str,
        plain_text: str,
        image_sources: List[str],
        use_markdown: bool,
        template_id: str,
        param_key: str,
        msg_id: Optional[str],
    ) -> None:
        """Sends message to QQ group with rich media and Markdown fallback."""
        media = None
        if image_sources:
            media = await self._resolve_and_upload_media(
                group_openid=group_openid,
                source=image_sources[0],
                file_type=1,
                is_group=True,
            )

        if media:
            await self.safe_post_group_message(
                group_openid=group_openid,
                msg_type=7,
                media=media,
                content=plain_text if plain_text else None,
                msg_id=msg_id,
            )
            return

        content_payload = plain_text or "(Empty response)"
        if use_markdown:
            try:
                md_payload = (
                    MarkdownPayload(
                        custom_template_id=template_id,
                        params=[{"key": param_key, "values": [content_payload]}],
                    )
                    if template_id
                    else MarkdownPayload(content=content_payload)
                )
                await self.safe_post_group_message(
                    group_openid=group_openid,
                    msg_type=2,
                    markdown=md_payload,
                    msg_id=msg_id,
                )
                return
            except Exception as e:
                err_str = str(e)
                if "40054005" in err_str or "markdown" in err_str.lower():
                    # Native Markdown not allowed; fallback gracefully to plain text
                    pass
                else:
                    raise

        # Plain text delivery
        await self.safe_post_group_message(
            group_openid=group_openid,
            msg_type=0,
            content=content_payload,
            msg_id=msg_id,
        )

    async def _deliver_c2c(
        self,
        user_openid: str,
        plain_text: str,
        image_sources: List[str],
        use_markdown: bool,
        template_id: str,
        param_key: str,
        msg_id: Optional[str],
    ) -> None:
        """Sends message to direct C2C user with rich media and Markdown fallback."""
        media = None
        if image_sources:
            media = await self._resolve_and_upload_media(
                group_openid=user_openid,
                source=image_sources[0],
                file_type=1,
                is_group=False,
            )

        if media:
            await self.safe_post_c2c_message(
                openid=user_openid,
                msg_type=7,
                media=media,
                content=plain_text if plain_text else None,
                msg_id=msg_id,
            )
            return

        content_payload = plain_text or "(Empty response)"
        if use_markdown:
            try:
                md_payload = (
                    MarkdownPayload(
                        custom_template_id=template_id,
                        params=[{"key": param_key, "values": [content_payload]}],
                    )
                    if template_id
                    else MarkdownPayload(content=content_payload)
                )
                await self.safe_post_c2c_message(
                    openid=user_openid,
                    msg_type=2,
                    markdown=md_payload,
                    msg_id=msg_id,
                )
                return
            except Exception as e:
                err_str = str(e)
                if "40054005" in err_str or "markdown" in err_str.lower():
                    pass
                else:
                    raise

        await self.safe_post_c2c_message(
            openid=user_openid,
            msg_type=0,
            content=content_payload,
            msg_id=msg_id,
        )

    async def _deliver_guild(
        self,
        channel_id: str,
        plain_text: str,
        image_sources: List[str],
        msg_id: Optional[str],
    ) -> None:
        """Sends message to guild channel."""
        if not self.bot_client:
            raise RuntimeError("QQ client disconnected")
        image_url = image_sources[0] if (image_sources and image_sources[0].startswith("http")) else None
        await self.bot_client.api.post_message(
            channel_id=channel_id,
            content=plain_text or "(Empty response)",
            image=image_url,
            msg_id=msg_id,
        )

    async def _deliver_guild_dm(
        self,
        guild_id: str,
        plain_text: str,
        msg_id: Optional[str],
    ) -> None:
        """Sends direct message in guild."""
        if not self.bot_client:
            raise RuntimeError("QQ client disconnected")
        await self.bot_client.api.post_dms(
            guild_id=guild_id,
            content=plain_text or "(Empty response)",
            msg_id=msg_id,
        )

    async def _resolve_and_upload_media(
        self,
        group_openid: str,
        source: str,
        file_type: int,
        is_group: bool,
    ) -> Media:
        """Resolves one media source into a QQ ``media`` payload.

        A local file is always uploaded with the chunked protocol: the inline endpoint only accepts
        a publicly reachable ``url`` (and its ``file_data`` mode is still unsupported upstream), so
        handing it a path fails with ``40093010 上传URL错误``. An ``http(s)`` source is a real URL
        and goes through the inline endpoint, which skips the extra round-trips.
        """
        if not self.bot_client:
            raise RuntimeError("QQ client disconnected")

        path = Path(source)
        if path.is_file():
            if self.uploader is None:
                raise RuntimeError(
                    f"QQ uploader unavailable; cannot send local file {path.name}"
                )
            if is_group:
                return await self.uploader.upload_group(
                    file_path=path,
                    file_type=file_type,
                    file_name=path.name,
                    group_openid=group_openid,
                )
            return await self.uploader.upload_c2c(
                file_path=path,
                file_type=file_type,
                file_name=path.name,
                user_openid=group_openid,
            )

        if not source.startswith(("http://", "https://")):
            raise RuntimeError(
                f"Media source '{source}' is neither a readable file nor an http(s) URL"
            )

        if is_group:
            return await self.bot_client.api.post_group_file(
                group_openid=group_openid,
                file_type=file_type,
                url=source,
                srv_send_msg=False,
            )
        return await self.bot_client.api.post_c2c_file(
            openid=group_openid,
            file_type=file_type,
            url=source,
            srv_send_msg=False,
        )
