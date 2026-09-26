"""Unit tests for Kanon QQ Official platform adapter plugin."""

import asyncio
from pathlib import Path
from unittest import IsolatedAsyncioTestCase
from unittest.mock import AsyncMock, MagicMock, patch

import sys
# Ensure sdks/python and plugins/qqofficial are on sys.path
_repo_root = Path(__file__).resolve().parents[3]
_python_sdk_dir = _repo_root / "sdks" / "python"
_plugin_dir = _repo_root / "plugins" / "qqofficial"
for p in (str(_python_sdk_dir), str(_plugin_dir)):
    if p not in sys.path:
        sys.path.insert(0, p)

from kanon_sdk.context import MessageSegment, PluginContext
from kanon_sdk.proto import pb
from main import KanonBotClient, QQOfficialAdapter


class TestQQOfficialAdapter(IsolatedAsyncioTestCase):
    """Tests for QQOfficialAdapter and KanonBotClient."""

    def setUp(self) -> None:
        self.adapter = QQOfficialAdapter()

    def test_adapter_metadata(self) -> None:
        """Verifies static plugin metadata declarations."""
        self.assertEqual(self.adapter.id, "org.kanon.adapter.qqofficial")
        self.assertEqual(self.adapter.name, "QQ Official Adapter")
        meta = self.adapter.meta()
        self.assertEqual(meta.id, "org.kanon.adapter.qqofficial")
        self.assertEqual(meta.name, "QQ Official Adapter")

    async def test_on_load_missing_credentials(self) -> None:
        """Verifies idle state when appid/secret are omitted in config."""
        ctx = PluginContext(data_dir=Path("/tmp/kanon_test"), config={})
        await self.adapter.on_load(ctx)
        self.assertIsNone(self.adapter.bot_client)

    async def test_on_deliver_without_client(self) -> None:
        """Verifies explicit failure when delivering message while disconnected."""
        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="group:12345",
            recipient_id="user_1",
            segments=[MessageSegment.text("Hello")],
            event_id="msg_001",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertFalse(resp.success)
        self.assertIn("disconnected", resp.error_message)

    async def test_inbound_group_at_message_create(self) -> None:
        """Verifies inbound group @ mention event extraction and Fast-ACK ingestion."""
        mock_core = MagicMock()
        mock_core.ingest_event = AsyncMock(return_value=pb.IngestEventResponse(accepted=True, event_id="evt_01"))
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), core=mock_core)

        client = KanonBotClient(adapter=self.adapter)

        # Mock GroupMessage structure
        mock_msg = MagicMock()
        mock_msg.group_openid = "test_group_openid_123"
        mock_msg.author.member_openid = "test_member_openid_456"
        mock_msg.content = "  /help command  "
        mock_msg.id = "qq_msg_group_999"
        mock_msg.mentions = []

        await client.on_group_at_message_create(mock_msg)

        mock_core.ingest_event.assert_awaited_once_with(
            platform="qqofficial",
            channel_id="group:test_group_openid_123",
            sender_id="test_member_openid_456",
            text="/help command",
            event_id="qq_msg_group_999",
            metadata={"msg_id": "qq_msg_group_999", "scene": "group", "mentions": []},
        )

    async def test_inbound_c2c_message_create(self) -> None:
        """Verifies inbound private direct message (C2C) event extraction."""
        mock_core = MagicMock()
        mock_core.ingest_event = AsyncMock(return_value=pb.IngestEventResponse(accepted=True, event_id="evt_02"))
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), core=mock_core)

        client = KanonBotClient(adapter=self.adapter)

        # Mock C2CMessage structure
        mock_msg = MagicMock()
        mock_msg.author.user_openid = "test_user_openid_789"
        mock_msg.content = "ping"
        mock_msg.id = "qq_msg_c2c_888"

        await client.on_c2c_message_create(mock_msg)

        mock_core.ingest_event.assert_awaited_once_with(
            platform="qqofficial",
            channel_id="c2c:test_user_openid_789",
            sender_id="test_user_openid_789",
            text="ping",
            event_id="qq_msg_c2c_888",
            metadata={"msg_id": "qq_msg_c2c_888", "scene": "c2c"},
        )

    async def test_inbound_at_message_create_guild(self) -> None:
        """Verifies inbound guild channel @ mention event extraction."""
        mock_core = MagicMock()
        mock_core.ingest_event = AsyncMock(return_value=pb.IngestEventResponse(accepted=True, event_id="evt_03"))
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), core=mock_core)

        client = KanonBotClient(adapter=self.adapter)

        # Mock guild Message structure
        mock_msg = MagicMock()
        mock_msg.channel_id = "test_guild_chan_111"
        mock_msg.guild_id = "test_guild_999"
        mock_msg.author.id = "test_guild_author_222"
        mock_msg.content = "hello guild"
        mock_msg.id = "qq_msg_guild_777"

        await client.on_at_message_create(mock_msg)

        mock_core.ingest_event.assert_awaited_once_with(
            platform="qqofficial",
            channel_id="guild:test_guild_chan_111",
            sender_id="test_guild_author_222",
            text="hello guild",
            event_id="qq_msg_guild_777",
            metadata={"msg_id": "qq_msg_guild_777", "scene": "guild", "guild_id": "test_guild_999"},
        )

    async def test_deliver_group_message(self) -> None:
        """Verifies outbound delivery to group channel."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        mock_client.api.post_group_message = AsyncMock(return_value={"id": "qq_resp_1"})
        self.adapter.bot_client = mock_client
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), config={"use_markdown": False})

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="group:grp_abc",
            recipient_id="user_xyz",
            segments=[MessageSegment.text("Group reply text")],
            event_id="evt_reply_1",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertTrue(resp.success)
        self.assertEqual(resp.message_id, "evt_reply_1")

        mock_client.api.post_group_message.assert_awaited_once_with(
            group_openid="grp_abc",
            msg_type=0,
            content="Group reply text",
            msg_id="evt_reply_1",
            msg_seq=1,
        )

    async def test_deliver_group_message_with_image(self) -> None:
        """Verifies outbound delivery to group with uploaded image."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        fake_media = MagicMock()
        mock_client.api.post_group_file = AsyncMock(return_value=fake_media)
        mock_client.api.post_group_message = AsyncMock(return_value={"id": "qq_resp_2"})
        self.adapter.bot_client = mock_client

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="group:grp_img",
            segments=[
                MessageSegment.text("Look at this image:"),
                MessageSegment.image_url("https://example.com/pic.jpg"),
            ],
            event_id="evt_reply_img",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertTrue(resp.success)

        mock_client.api.post_group_file.assert_awaited_once_with(
            group_openid="grp_img",
            file_type=1,
            url="https://example.com/pic.jpg",
            srv_send_msg=False,
        )
        mock_client.api.post_group_message.assert_awaited_once_with(
            group_openid="grp_img",
            msg_type=7,
            media=fake_media,
            content="Look at this image:",
            msg_id="evt_reply_img",
            msg_seq=1,
        )

    async def test_deliver_c2c_message(self) -> None:
        """Verifies outbound delivery to direct c2c conversation."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        mock_client.api.post_c2c_message = AsyncMock(return_value={"id": "qq_resp_3"})
        self.adapter.bot_client = mock_client
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), config={"use_markdown": False})

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="c2c:user_direct_1",
            segments=[MessageSegment.text("Direct message hello")],
            event_id="evt_c2c_1",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertTrue(resp.success)
        mock_client.api.post_c2c_message.assert_awaited_once_with(
            openid="user_direct_1",
            msg_type=0,
            content="Direct message hello",
            msg_id="evt_c2c_1",
            msg_seq=1,
        )

    async def test_deliver_guild_message(self) -> None:
        """Verifies outbound delivery to guild channel."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        mock_client.api.post_message = AsyncMock(return_value={"id": "qq_resp_4"})
        self.adapter.bot_client = mock_client

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="guild:chan_guild_1",
            segments=[
                MessageSegment.text("Guild response"),
                MessageSegment.image_url("https://example.com/guild.png"),
            ],
            event_id="evt_guild_1",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertTrue(resp.success)
        mock_client.api.post_message.assert_awaited_once_with(
            channel_id="chan_guild_1",
            content="Guild response",
            image="https://example.com/guild.png",
            msg_id="evt_guild_1",
        )

    async def test_deliver_unsupported_channel_scheme(self) -> None:
        """Verifies rejection of unsupported channel prefix."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        self.adapter.bot_client = mock_client

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="unknown_scheme:12345",
            segments=[MessageSegment.text("Test")],
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertFalse(resp.success)
        self.assertIn("Unsupported channel scheme", resp.error_message)

    async def test_tool_request_login_qr(self) -> None:
        """Verifies qq_request_login_qr tool execution."""
        from auth import QQOfficialLoginRegistration

        fake_reg = QQOfficialLoginRegistration(
            task_id="task_qr_123",
            bind_key="bind_key_456",
            qrcode="https://q.qq.com/connect?task_id=task_qr_123",
            interval=2,
        )
        with patch("main.request_qqofficial_login_qr", AsyncMock(return_value=fake_reg)):
            res = await self.adapter.handle_request_login_qr({})
            self.assertEqual(res["task_id"], "task_qr_123")
            self.assertEqual(res["bind_key"], "bind_key_456")
            self.assertEqual(res["qrcode_url"], "https://q.qq.com/connect?task_id=task_qr_123")
            self.assertEqual(res["poll_interval_seconds"], 2)

    async def test_tool_poll_login_result(self) -> None:
        """Verifies qq_poll_login_result tool execution and credential sync."""
        from unittest.mock import patch

        fake_poll = {
            "status": "created",
            "qr_status": 2,
            "appid": "102888123",
            "secret": "decrypted_secret_val",
        }
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), config={})
        with patch("main.poll_qqofficial_login_once", AsyncMock(return_value=fake_poll)):
            res = await self.adapter.handle_poll_login_result({"task_id": "t1", "bind_key": "k1"})
            self.assertEqual(res["status"], "created")
            self.assertEqual(res["appid"], "102888123")
            self.assertEqual(res["secret"], "decrypted_secret_val")
            self.assertEqual(self.adapter.context.config["appid"], "102888123")
            self.assertEqual(self.adapter.context.config["secret"], "decrypted_secret_val")

    async def test_deliver_markdown_auto_fallback_on_40054005(self) -> None:
        """Verifies automatic fallback to plain text when QQ API rejects native Markdown (40054005)."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        # Markdown post fails with 40054005 permission error
        mock_client.api.post_group_message = AsyncMock(side_effect=[
            RuntimeError("QQ API Error: 40054005 不允许发送原生 markdown"),
            {"id": "qq_fallback_ok"},
        ])
        self.adapter.bot_client = mock_client
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), config={"use_markdown": True})

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="group:grp_fallback",
            recipient_id="user_xyz",
            segments=[MessageSegment.text("**Bold message**")],
            event_id="evt_fb_1",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertTrue(resp.success)
        self.assertEqual(mock_client.api.post_group_message.await_count, 2)
        # Second call is plain text msg_type=0
        fallback_call = mock_client.api.post_group_message.await_args_list[1]
        self.assertEqual(fallback_call.kwargs["msg_type"], 0)
        self.assertEqual(fallback_call.kwargs["content"], "**Bold message**")

    async def test_on_config_reload_starts_bot_client(self) -> None:
        """Verifies hot reload starts or restarts the bot client when credentials are provided."""
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), config={})
        self.assertIsNone(self.adapter.bot_client)

        with patch.object(self.adapter, "_start_bot_client", AsyncMock()) as mock_start:
            await self.adapter.on_config_reload({"appid": "12345", "secret": "abcde"})
            mock_start.assert_awaited_once()

    async def test_deliver_group_msg_id_rejected_fallback_active(self) -> None:
        """Verifies automatic fallback to active message when passive msg_id is rejected by QQ."""
        mock_client = MagicMock()
        mock_client.is_closed.return_value = False
        mock_client.api.post_group_message = AsyncMock(side_effect=[
            RuntimeError("botpy.errors.ServerError: 请求参数msg_id无效或越权 (code: 304023)"),
            {"id": "qq_active_ok"},
        ])
        self.adapter.bot_client = mock_client
        self.adapter.context = PluginContext(data_dir=Path("/tmp"), config={"use_markdown": False})

        req = pb.DeliverMessageRequest(
            platform="qqofficial",
            channel_id="group:grp_test",
            recipient_id="user_xyz",
            segments=[MessageSegment.text("Reply after timeout")],
            event_id="expired_msg_id",
        )
        resp = await self.adapter.on_deliver_message(req)
        self.assertTrue(resp.success)
        self.assertEqual(mock_client.api.post_group_message.await_count, 2)
        # Second call should retry with msg_id=None
        second_call = mock_client.api.post_group_message.await_args_list[1]
        self.assertIsNone(second_call.kwargs["msg_id"])
        self.assertEqual(second_call.kwargs["content"], "Reply after timeout")

    async def test_bot_client_on_ready(self) -> None:
        """Verifies on_ready callback executes and logs bot online status without crashing."""
        client = KanonBotClient(adapter=self.adapter)
        client._connection = MagicMock()
        client._connection.state.robot.name = "TestRobot"
        await client.on_ready()

