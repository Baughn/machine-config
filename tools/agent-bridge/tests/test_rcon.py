"""The rcon tool against a fake RCON server speaking the real wire protocol."""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator
from pathlib import Path

import pytest

from agent_bridge import rcon
from agent_bridge.approval import Verdict
from agent_bridge.session import ToolError

from conftest import ALICE, Harness


class FakeServer:
    def __init__(self, password: str) -> None:
        self.password = password
        self.commands: list[str] = []
        self.port = 0

    async def handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        request_id, kind, body = await rcon.read_packet(reader)
        assert kind == rcon.LOGIN
        writer.write(rcon.packet(request_id if body == self.password else -1, 2, ""))
        await writer.drain()
        if body != self.password:
            writer.close()
            return
        request_id, kind, body = await rcon.read_packet(reader)
        self.commands.append(body)
        reply = "There are 1 of a max of 20 players online: maxwell_lt" if body == "list" else f"ran {body}"
        writer.write(rcon.packet(request_id, 0, reply))
        await writer.drain()
        writer.close()


@pytest.fixture
async def server(tmp_path: Path) -> AsyncIterator[FakeServer]:
    fake = FakeServer("hunter2")
    listener = await asyncio.start_server(fake.handle, "127.0.0.1", 0)
    fake.port = listener.sockets[0].getsockname()[1]
    world = tmp_path / "worlds" / "erisia"
    world.mkdir(parents=True)
    (world / "server.properties").write_text(
        f"#comment\nenable-rcon=true\nrcon.port={fake.port}\nrcon.password=hunter2\nmotd=a=b\n")
    yield fake
    listener.close()


def rcon_harness(tmp_path: Path, **overrides: object) -> Harness:
    settings: dict[str, object] = {"rcon_root": str(tmp_path / "worlds"),
                                   "rcon_read_only": ["list", "forge tps", "spark tps"], **overrides}
    return Harness(tmp_path, **settings)


def test_matches_leading_words() -> None:
    allowed = ("list", "forge tps")
    assert rcon.matches("list", allowed)
    assert rcon.matches("list uuids", allowed)
    assert rcon.matches("forge tps 0", allowed)
    assert not rcon.matches("forge", allowed)
    assert not rcon.matches("listen", allowed)
    assert not rcon.matches("say list", allowed)
    assert not rcon.matches("op mallory", allowed)


def test_target_validation(tmp_path: Path) -> None:
    for name in ("../etc", "Erisia", "a/b", ""):
        with pytest.raises(rcon.RconError, match="not a world"):
            rcon.target(tmp_path, name)
    with pytest.raises(rcon.RconError, match="cannot read"):
        rcon.target(tmp_path, "missing")
    (tmp_path / "off").mkdir()
    (tmp_path / "off/server.properties").write_text("enable-rcon=false\n")
    with pytest.raises(rcon.RconError, match="not enabled"):
        rcon.target(tmp_path, "off")


async def test_read_only_command_runs_at_once(tmp_path: Path, server: FakeServer) -> None:
    h = rcon_harness(tmp_path)
    reply = await h.bridge.tool_rcon({"world": "erisia", "command": "/list"})
    assert reply.startswith("There are 1 of a max of 20 players online: maxwell_lt")
    assert server.commands == ["list"]
    assert h.chat.approvals == []


async def test_other_commands_need_an_approver(tmp_path: Path, server: FakeServer) -> None:
    h = rcon_harness(tmp_path)
    call = asyncio.create_task(h.bridge.tool_rcon({"world": "erisia", "command": "say hello"}))
    for _ in range(20):
        await asyncio.sleep(0)
    assert server.commands == []
    [request] = h.chat.approvals
    assert '"command": "say hello"' in h.chat.sent[request].content
    await h.bridge.on_decide(request, h.author(ALICE), Verdict.ALLOW)
    assert (await call).startswith("ran say hello")
    assert server.commands == ["say hello"]

    call = asyncio.create_task(h.bridge.tool_rcon({"world": "erisia", "command": "op mallory"}))
    for _ in range(20):
        await asyncio.sleep(0)
    await h.bridge.on_decide(h.chat.approvals[-1], h.author(ALICE), Verdict.DENY)
    with pytest.raises(ToolError, match="Denied by alice"):
        await call
    assert server.commands == ["say hello"]


async def test_wrong_password_and_down_server(tmp_path: Path, server: FakeServer) -> None:
    h = rcon_harness(tmp_path)
    server.password = "other"
    with pytest.raises(ToolError, match="rejected the password"):
        await h.bridge.tool_rcon({"world": "erisia", "command": "list"})
    props = tmp_path / "worlds/erisia/server.properties"
    props.write_text(props.read_text().replace(f"rcon.port={server.port}", "rcon.port=1"))
    with pytest.raises(ToolError, match="connection failed"):
        await h.bridge.tool_rcon({"world": "erisia", "command": "list"})


async def test_rcon_is_off_unless_configured(harness: Harness) -> None:
    with pytest.raises(ToolError, match="not configured"):
        await harness.bridge.tool_rcon({"world": "erisia", "command": "list"})


async def test_auto_mode_leaves_rcon_to_the_classifier_except_the_ask_list(tmp_path: Path, server: FakeServer) -> None:
    h = rcon_harness(tmp_path, permission_mode="auto", rcon_ask=["stop", "op", "whitelist"],
                     rcon_read_only=["list", "whitelist list"])
    assert (await h.bridge.tool_rcon({"world": "erisia", "command": "forge entity list"})).startswith("ran forge")
    assert (await h.bridge.tool_rcon({"world": "erisia", "command": "whitelist list"})).startswith("ran whitelist")
    assert h.chat.approvals == []
    for text in ("op mallory", "whitelist remove bob", "stop"):
        call = asyncio.create_task(h.bridge.tool_rcon({"world": "erisia", "command": text}))
        for _ in range(20):
            await asyncio.sleep(0)
        assert not call.done(), text
        await h.bridge.on_decide(h.chat.approvals[-1], h.author(ALICE), Verdict.DENY)
        with pytest.raises(ToolError):
            await call
    assert server.commands == ["forge entity list", "whitelist list"]


def test_rcon_is_allow_listed_only_outside_auto_mode() -> None:
    from agent_bridge.session import RCON_TOOL, options_kwargs
    base = dict(workdir=Path("/w"), state=Path("/s"), cli_path=None, model=None, resume=None,
                system_prompt="", allow=(), ask=(), deny=(), token="t")
    assert RCON_TOOL in options_kwargs(permission_mode="default", **base)["allowed_tools"]  # type: ignore[arg-type]
    assert RCON_TOOL not in options_kwargs(permission_mode="auto", **base)["allowed_tools"]  # type: ignore[arg-type]
