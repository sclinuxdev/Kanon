"""WebSocket Gateway client for Kanon QQ Official adapter.

Encapsulates botpy.Client event callbacks and routes inbound messages and notices
to the Kanon Core pipeline with Fast-ACK guarantees.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Dict, List, Optional

import botpy
try:
    from botpy.message import C2CMessage, DirectMessage, GroupMessage, Message
except ImportError:
    from botpy.types.message import C2CMessage, DirectMessage, GroupMessage, Message

if TYPE_CHECKING:
    from main import QQOfficialAdapter


class KanonBotClient(botpy.Client):
    """QQ Official bot client bridging WebSocket gateway events to Kanon Core."""

    def __init__(self, adapter: Any, *args: Any, **kwargs: Any) -> None:
        if "intents" not in kwargs and not args:
            kwargs["intents"] = botpy.Intents.none()
        super().__init__(*args, **kwargs)
        self.adapter = adapter

    async def on_ready(self) -> None:
        """Invoked when WebSocket gateway connection and handshake succeed."""
        bot_name = getattr(getattr(self, "robot", None), "name", "QQ Bot")
        print(
            f"[QQOfficial] QQ Bot '{bot_name}' connected to gateway successfully! "
            "Online and listening for events.",
            flush=True,
        )

    async def on_group_at_message_create(self, message: GroupMessage) -> None:
        """Handles group @ mentions."""
        content = (message.content or "").strip()
        print(
            f"[QQOfficial] Received Group @ Message from member={message.author.member_openid} "
            f"in group={message.group_openid}: '{content}' (id={message.id})",
            flush=True,
        )
        mentions: List[str] = [
            getattr(m, "member_openid", "")
            for m in getattr(message, "mentions", [])
        ]
        await self.adapter.ingest_qq_message(
            channel_id=f"group:{message.group_openid}",
            sender_id=message.author.member_openid,
            content=content,
            msg_id=message.id,
            scene="group",
            extra={"mentions": mentions},
        )

    async def on_group_message_create(self, message: GroupMessage) -> None:
        """Handles unmentioned group messages for authorized private domain bots."""
        content = (message.content or "").strip()
        print(
            f"[QQOfficial] Received Group Message from member={message.author.member_openid} "
            f"in group={message.group_openid}: '{content}' (id={message.id})",
            flush=True,
        )
        await self.adapter.ingest_qq_message(
            channel_id=f"group:{message.group_openid}",
            sender_id=message.author.member_openid,
            content=content,
            msg_id=message.id,
            scene="group",
            extra={"unmentioned": True},
        )

    async def on_c2c_message_create(self, message: C2CMessage) -> None:
        """Handles direct private messages (C2C)."""
        content = (message.content or "").strip()
        print(
            f"[QQOfficial] Received C2C Private Message from user={message.author.user_openid}: "
            f"'{content}' (id={message.id})",
            flush=True,
        )
        await self.adapter.ingest_qq_message(
            channel_id=f"c2c:{message.author.user_openid}",
            sender_id=message.author.user_openid,
            content=content,
            msg_id=message.id,
            scene="c2c",
        )

    async def on_at_message_create(self, message: Message) -> None:
        """Handles guild channel @ mentions."""
        content = (message.content or "").strip()
        print(
            f"[QQOfficial] Received Guild @ Message from author={message.author.id} "
            f"in channel={message.channel_id}: '{content}' (id={message.id})",
            flush=True,
        )
        await self.adapter.ingest_qq_message(
            channel_id=f"guild:{message.channel_id}",
            sender_id=message.author.id,
            content=content,
            msg_id=message.id,
            scene="guild",
            extra={"guild_id": getattr(message, "guild_id", "")},
        )

    async def on_message_create(self, message: Message) -> None:
        """Handles unmentioned guild channel messages for authorized bots."""
        content = (message.content or "").strip()
        print(
            f"[QQOfficial] Received Guild Message from author={message.author.id} "
            f"in channel={message.channel_id}: '{content}' (id={message.id})",
            flush=True,
        )
        await self.adapter.ingest_qq_message(
            channel_id=f"guild:{message.channel_id}",
            sender_id=message.author.id,
            content=content,
            msg_id=message.id,
            scene="guild",
            extra={"guild_id": getattr(message, "guild_id", ""), "unmentioned": True},
        )

    async def on_direct_message_create(self, message: DirectMessage) -> None:
        """Handles direct messages within guild."""
        content = (message.content or "").strip()
        channel_id = getattr(message, "channel_id", "") or getattr(message, "guild_id", "")
        author_id = getattr(message.author, "id", "")
        print(
            f"[QQOfficial] Received Guild DM from author={author_id} "
            f"in channel={channel_id}: '{content}' (id={message.id})",
            flush=True,
        )
        await self.adapter.ingest_qq_message(
            channel_id=f"guild_dm:{channel_id}",
            sender_id=author_id,
            content=content,
            msg_id=message.id,
            scene="guild_dm",
        )

    async def on_group_add_robot(self, event: Any) -> None:
        """Handles bot added to QQ group notice."""
        group_openid = getattr(event, "group_openid", "")
        op_openid = getattr(event, "op_member_openid", "")
        await self.adapter.ingest_notice(
            channel_id=f"group:{group_openid}",
            event_type="group_add_robot",
            data={"group_openid": group_openid, "op_member_openid": op_openid},
        )

    async def on_group_del_robot(self, event: Any) -> None:
        """Handles bot removed from QQ group notice."""
        group_openid = getattr(event, "group_openid", "")
        await self.adapter.ingest_notice(
            channel_id=f"group:{group_openid}",
            event_type="group_del_robot",
            data={"group_openid": group_openid},
        )

    async def on_friend_add(self, event: Any) -> None:
        """Handles friend added notice."""
        openid = getattr(event, "openid", "")
        await self.adapter.ingest_notice(
            channel_id=f"c2c:{openid}",
            event_type="friend_add",
            data={"openid": openid},
        )

    async def on_friend_del(self, event: Any) -> None:
        """Handles friend removed notice."""
        openid = getattr(event, "openid", "")
        await self.adapter.ingest_notice(
            channel_id=f"c2c:{openid}",
            event_type="friend_del",
            data={"openid": openid},
        )
