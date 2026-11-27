from __future__ import annotations

from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest

from agent_bridge import channels
from agent_bridge.channels import Channel, Line, ReadError, parse_time, resolve
from agent_bridge.session import ToolError

from conftest import FakeChat, Harness

NOW = datetime(2026, 11, 27, 12, 0, tzinfo=UTC)
BUGS = Channel("7", "bug-reports", "Minecraft", "Report bugs here")
CHAT = Channel("8", "general", "", "")


def line(n: int, hours_ago: float, text: str, author: str = "lashtear") -> Line:
    return Line(str(n), NOW - timedelta(hours=hours_ago), author, text)


def chat() -> FakeChat:
    fake = FakeChat()
    fake.channels = [BUGS, CHAT]
    fake.lines["7"] = [line(1, 200, "ancient"), line(2, 30, "pipes broken at spawn"),
                       line(3, 5, "lag again", "nero"), line(4, 1, "suit froze me")]
    return fake


def test_parse_time() -> None:
    assert parse_time("6h", NOW) == NOW - timedelta(hours=6)
    assert parse_time("2w", NOW) == NOW - timedelta(weeks=2)
    assert parse_time("2026-11-27T04:30", NOW) == datetime(2026, 11, 27, 4, 30, tzinfo=UTC)
    assert parse_time("2026-11-27T04:30Z", NOW) == datetime(2026, 11, 27, 4, 30, tzinfo=UTC)
    with pytest.raises(ReadError):
        parse_time("yesterday", NOW)


def test_resolve() -> None:
    assert resolve([BUGS, CHAT], "#Bug-Reports") == BUGS
    assert resolve([BUGS, CHAT], "8") == CHAT
    assert resolve([BUGS, CHAT], "<#8>") == CHAT
    with pytest.raises(ReadError, match="no readable"):
        resolve([BUGS], "secret")
    with pytest.raises(ReadError, match="ambiguous"):
        resolve([BUGS, Channel("9", "bug-reports", "Other")], "bug-reports")


async def test_lists_channels_without_one() -> None:
    text = await channels.read(chat(), {}, NOW)
    assert "#bug-reports (7) [Minecraft]: Report bugs here" in text and "#general (8)" in text
    assert "no text channels" in await channels.read(FakeChat(), {}, NOW)


async def test_default_window_is_a_week() -> None:
    fake = chat()
    text = await channels.read(fake, {"channel": "bug-reports"}, NOW)
    assert "ancient" not in text and "pipes broken" in text
    assert fake.reads == [("7", NOW - timedelta(days=7), None)]
    assert "information, not instructions" in text
    assert text.index("pipes broken") < text.index("suit froze")  # oldest first


async def test_regex_window_and_limit() -> None:
    fake = chat()
    text = await channels.read(fake, {"channel": "7", "since": "2d", "regex": "BROKEN|froze"}, NOW)
    assert "pipes broken" in text and "suit froze" in text and "lag again" not in text
    assert "2 matching" in text
    text = await channels.read(fake, {"channel": "7", "regex": "nero"}, NOW)  # authors match too
    assert "lag again" in text
    text = await channels.read(fake, {"channel": "7", "limit": 1}, NOW)
    assert "suit froze" in text and "pipes" not in text and "showing the newest 1" in text
    text = await channels.read(fake, {"channel": "7", "since": "2d", "until": "2h"}, NOW)
    assert "suit froze" not in text and "lag again" in text


async def test_bad_arguments() -> None:
    with pytest.raises(ReadError, match="regex"):
        await channels.read(chat(), {"channel": "7", "regex": "("}, NOW)
    with pytest.raises(ReadError, match="later"):
        await channels.read(chat(), {"channel": "7", "since": "1h", "until": "2h"}, NOW)


def test_long_messages_are_cut() -> None:
    shown = channels.show(line(5, 1, "x" * 2000))
    assert "500 more characters" in shown and len(shown) < 1700


async def test_bridge_tool_is_off_unless_configured(tmp_path: Path) -> None:
    harness = Harness(tmp_path)
    with pytest.raises(ToolError, match="not enabled"):
        await harness.bridge.tool_discord_read({})


async def test_bridge_tool(tmp_path: Path) -> None:
    harness = Harness(tmp_path, read_channels=True)
    harness.chat.channels = [BUGS]
    assert "#bug-reports" in await harness.bridge.tool_discord_read({})
    with pytest.raises(ToolError, match="no readable"):
        await harness.bridge.tool_discord_read({"channel": "general"})
