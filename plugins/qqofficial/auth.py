"""QQ Official Bot QR login and credentials binding manager.

Provides AES-256 GCM key generation, QR task creation, and result polling.
"""

from __future__ import annotations

import base64
import secrets
from dataclasses import dataclass
from typing import Any
from urllib.parse import quote

import httpx
from Crypto.Cipher import AES

DEFAULT_QQOFFICIAL_BIND_HOST = "q.qq.com"
DEFAULT_QQOFFICIAL_QR_POLL_INTERVAL = 2
DEFAULT_QQOFFICIAL_API_TIMEOUT_MS = 10_000

QQOFFICIAL_BIND_STATUS_NONE = 0
QQOFFICIAL_BIND_STATUS_PENDING = 1
QQOFFICIAL_BIND_STATUS_COMPLETED = 2
QQOFFICIAL_BIND_STATUS_EXPIRED = 3


@dataclass
class QQOfficialLoginRegistration:
    """Represents an active QR binding registration task."""
    task_id: str
    bind_key: str
    qrcode: str
    interval: int


def _string_field(data: dict[str, Any], key: str) -> str:
    value = data.get(key)
    if isinstance(value, str):
        return value.strip()
    return ""


def _int_config(value: Any, default: int, minimum: int) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        parsed = default
    return max(parsed, minimum)


def _bind_host(platform_config: dict[str, Any]) -> str:
    host = _string_field(platform_config, "qqofficial_bind_host")
    if not host:
        host = DEFAULT_QQOFFICIAL_BIND_HOST
    host = host.removeprefix("https://").removeprefix("http://").rstrip("/")
    return host or DEFAULT_QQOFFICIAL_BIND_HOST


def _connect_url(task_id: str, host: str) -> str:
    return f"https://{host}/qqbot/openclaw/connect.html?task_id={quote(task_id, safe='')}&_wv=2"


async def _post_json(*, url: str, payload: dict[str, Any], timeout_ms: int) -> dict[str, Any]:
    async with httpx.AsyncClient(timeout=timeout_ms / 1000) as client:
        response = await client.post(url, json=payload, headers={"Accept": "application/json"})
        response.raise_for_status()
        data = response.json()
    if not isinstance(data, dict):
        raise RuntimeError("QQ Official bot binding API returned non-dict payload")
    retcode = data.get("retcode")
    if retcode is not None:
        try:
            retcode_ok = int(retcode) == 0
        except (TypeError, ValueError):
            retcode_ok = False
        if retcode_ok:
            return data
        message = _string_field(data, "msg") or _string_field(data, "message") or "QQ Official binding API failed"
        raise RuntimeError(message)
    return data


def generate_qqofficial_bind_key() -> str:
    """Generates a base64-encoded AES-256 key for QQ bot binding."""
    return base64.b64encode(secrets.token_bytes(32)).decode("ascii")


def decrypt_qqofficial_secret(encrypted_secret: str, bind_key: str) -> str:
    """Decrypts the AppSecret returned by QQ bot QR binding using AES-256-GCM."""
    try:
        key = base64.b64decode(bind_key)
        raw = base64.b64decode(encrypted_secret)
    except Exception as exc:
        raise ValueError("Failed to decode base64 bot credential") from exc
    if len(key) != 32 or len(raw) <= 28:
        raise ValueError("Invalid encrypted bot credential format")

    nonce = raw[:12]
    tag = raw[-16:]
    ciphertext = raw[12:-16]
    cipher = AES.new(key, AES.MODE_GCM, nonce=nonce)
    try:
        return cipher.decrypt_and_verify(ciphertext, tag).decode("utf-8")
    except Exception as exc:
        raise ValueError("Failed to decrypt and verify bot credential") from exc


def qqofficial_login_result(data: dict[str, Any], *, bind_key: str) -> dict[str, Any]:
    """Maps QQ bot bind polling response to standardized registration status."""
    payload = data.get("data")
    if not isinstance(payload, dict):
        payload = {}

    try:
        raw_status = int(payload.get("status", QQOFFICIAL_BIND_STATUS_NONE))
    except (TypeError, ValueError):
        raw_status = QQOFFICIAL_BIND_STATUS_NONE

    if raw_status == QQOFFICIAL_BIND_STATUS_COMPLETED:
        appid = str(payload.get("bot_appid") or "").strip()
        encrypted_secret = str(payload.get("bot_encrypt_secret") or "").strip()
        if not appid or not encrypted_secret:
            return {
                "status": "error",
                "qr_status": raw_status,
                "message": "Scan completed but incomplete credentials were returned",
            }
        try:
            secret = decrypt_qqofficial_secret(encrypted_secret, bind_key)
        except ValueError as exc:
            return {
                "status": "error",
                "qr_status": raw_status,
                "message": str(exc),
            }
        return {
            "status": "created",
            "qr_status": raw_status,
            "appid": appid,
            "secret": secret,
        }

    if raw_status == QQOFFICIAL_BIND_STATUS_EXPIRED:
        return {
            "status": "expired",
            "qr_status": raw_status,
            "message": "QR code expired",
        }

    return {"status": "pending", "qr_status": raw_status}


async def request_qqofficial_login_qr(
    platform_config: dict[str, Any],
) -> QQOfficialLoginRegistration:
    """Requests a QR binding task for QQ Official Bot credentials."""
    host = _bind_host(platform_config)
    timeout_ms = _int_config(platform_config.get("qqofficial_api_timeout_ms"), DEFAULT_QQOFFICIAL_API_TIMEOUT_MS, 1_000)
    interval = _int_config(platform_config.get("qqofficial_qr_poll_interval"), DEFAULT_QQOFFICIAL_QR_POLL_INTERVAL, 1)
    bind_key = generate_qqofficial_bind_key()
    data = await _post_json(
        url=f"https://{host}/lite/create_bind_task",
        payload={"key": bind_key},
        timeout_ms=timeout_ms,
    )

    payload = data.get("data")
    if not isinstance(payload, dict):
        payload = {}
    task_id = str(payload.get("task_id") or "").strip()
    if not task_id:
        raise RuntimeError("QQ bot binding response missing task_id")

    return QQOfficialLoginRegistration(
        task_id=task_id,
        bind_key=bind_key,
        qrcode=_connect_url(task_id, host),
        interval=interval,
    )


async def poll_qqofficial_login_once(
    *,
    platform_config: dict[str, Any],
    task_id: str,
    bind_key: str,
) -> dict[str, Any]:
    """Polls a QQ Official Bot QR binding task once."""
    if not task_id:
        raise ValueError("Missing task_id")
    if not bind_key:
        raise ValueError("Missing bind_key")

    host = _bind_host(platform_config)
    timeout_ms = _int_config(platform_config.get("qqofficial_api_timeout_ms"), DEFAULT_QQOFFICIAL_API_TIMEOUT_MS, 1_000)
    data = await _post_json(
        url=f"https://{host}/lite/poll_bind_result",
        payload={"task_id": task_id},
        timeout_ms=timeout_ms,
    )
    return qqofficial_login_result(data, bind_key=bind_key)
