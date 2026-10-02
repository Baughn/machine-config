from __future__ import annotations

from collections.abc import AsyncIterator
from pathlib import Path
from typing import Any

from aiohttp import web
import pytest

from agent_bridge.board import (BoardClient, BoardConfig, BoardError, BoardTools, briefing_text,
                                search_text, thread_line, unix)
from agent_bridge.bridge import Bridge
from agent_bridge.config import Board, ConfigError, parse
from agent_bridge.prompt import system_prompt
from agent_bridge.render import Roots
from agent_bridge.session import ToolError, options_kwargs
from conftest import ALICE, FakeChat, FakeSession, Harness, config_data, make_config

THREAD = {"id": 7, "title": "Autosave spike", "status": "open", "owner": "tsugumi-lab",
          "tags": ["perf"], "created": 1790946000, "updated": 1790949600, "due": "2026-10-04",
          "waiting_on": "baughn", "waiting_ref": "1555557513401860197",
          "summary": "Buffer the writes.\nNext: prototype.", "summary_author": "tsugumi-lab",
          "summary_updated": 1790949600, "summary_revisions": 2}
EMPTY_BRIEFING = {"agent": "x", "since": None, "asks": [], "waiting_on_you": [],
                  "waiting_on_others": [], "due": [], "changed": []}


class FakeBoard:
    """The board's HTTP API on a unix socket, recording what it was asked."""

    def __init__(self) -> None:
        self.requests: list[tuple[str, str, dict[str, str], Any]] = []
        self.briefing: dict[str, Any] = EMPTY_BRIEFING

    async def handle(self, request: web.Request) -> web.Response:
        body = await request.json() if request.can_read_body else None
        self.requests.append((request.method, request.path, dict(request.query), body))
        path = request.path
        if path == "/briefing":
            return web.json_response(self.briefing)
        if path == "/threads" and request.method == "GET":
            return web.json_response([THREAD])
        if path == "/threads" and request.method == "POST":
            return web.json_response({"thread": 8, "post": 21})
        if path == "/threads/7/posts":
            return web.json_response({"thread": 7, "post": 22})
        if path == "/threads/7" and request.method == "PATCH":
            return web.json_response({**THREAD, **(body or {})})
        if path == "/threads/99":
            return web.json_response({"error": "thread 99 not found"}, status=404)
        return web.json_response({"error": "unexpected"}, status=500)


@pytest.fixture
async def board(tmp_path: Path) -> AsyncIterator[tuple[FakeBoard, Path]]:
    fake = FakeBoard()
    app = web.Application()
    app.router.add_route("*", "/{tail:.*}", fake.handle)
    runner = web.AppRunner(app)
    await runner.setup()
    socket = tmp_path / "api.sock"
    await web.UnixSite(runner, str(socket)).start()
    yield fake, socket
    await runner.cleanup()


def tools_for(socket: Path, tmp_path: Path) -> BoardTools:
    return BoardTools(BoardClient(BoardConfig(socket=socket)), "tsugumi-lab", Roots((tmp_path,), 8 << 20))


def test_text_output() -> None:
    line = thread_line(THREAD)
    assert line.startswith("#7 Autosave spike [perf] - open, owner tsugumi-lab")
    assert "waiting on baughn (1555557513401860197); due 2026-10-04" in line
    assert line.endswith("\n    Buffer the writes.")
    assert briefing_text(EMPTY_BRIEFING) == "Board: nothing is waiting for you."
    ask = {"post": 3, "thread": 7, "thread_title": "Autosave spike", "author": "tsugumi-minecraft",
           "created": 1790946000, "first_line": "Can you measure it?"}
    text = briefing_text({**EMPTY_BRIEFING, "asks": [ask], "due": [THREAD]})
    assert text.index("post #3 in #7") < text.index("Open threads with a due date")
    assert "Threads waiting on you" not in text
    hits = [{"kind": "discord", "id": 1555569477968724201, "thread": None, "thread_title": None,
             "author": "baughn", "created": 1790946000, "snippet": "Cleaned-up\n[history]",
             "superseded": False}]
    assert search_text(hits) == ("discord 1555569477968724201 by baughn, 2026-10-02 13:00Z: "
                                 "Cleaned-up [history]")


def test_times() -> None:
    assert unix("2026-10-02", "since") == 1790899200
    assert unix("2026-10-02T13:00Z", "since") == 1790946000
    with pytest.raises(BoardError):
        unix("yesterday", "since")


async def test_tools_call_the_api(board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    fake, socket = board
    tools = tools_for(socket, tmp_path)
    assert "#7 Autosave spike" in await tools.call("board_list", {"mine": True, "updated_since": "2026-10-02"})
    assert fake.requests[-1][2] == {"involved": "tsugumi-lab", "updated_since": "1790899200"}

    (tmp_path / "results.md").write_text("32k writes")
    result = await tools.call("board_post", {
        "body": "Measured.", "new_thread": {"title": "Spike", "tags": ["perf"]}, "ask": "baughn",
        "attachments": [{"name": "results.md", "path": str(tmp_path / "results.md")}]})
    assert result == "opened thread #8 with post #21"
    body = fake.requests[-1][3]
    assert body["title"] == "Spike" and body["post"]["ask"] == "baughn"
    assert body["post"]["attachments"] == [{"name": "results.md", "content": "32k writes"}]

    assert await tools.call("board_post", {"body": "x", "thread": 7, "supersedes": ["3"]}) == \
        "posted #22 in thread #7"
    assert fake.requests[-1][3]["supersedes"] == [3]
    updated = await tools.call("board_summary", {"thread": 7, "summary": "Done.", "status": "resolved"})
    assert updated.startswith("updated: #7") and fake.requests[-1][3] == {"summary": "Done.", "status": "resolved"}


async def test_tools_report_errors(board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    _, socket = board
    tools = tools_for(socket, tmp_path)
    with pytest.raises(BoardError, match="thread 99 not found"):
        await tools.call("board_read", {"thread": 99})
    with pytest.raises(BoardError, match="exactly one of thread and new_thread"):
        await tools.call("board_post", {"body": "x"})
    with pytest.raises(BoardError, match="outside the workdir"):
        await tools.call("board_post", {"body": "x", "thread": 7,
                                        "attachments": [{"name": "p.txt", "path": "/etc/passwd"}]})
    (tmp_path / "blob.bin").write_bytes(b"\xff\xfe\x00")
    with pytest.raises(BoardError, match="isn't UTF-8"):
        await tools.call("board_post", {"body": "x", "thread": 7,
                                        "attachments": [{"name": "blob.bin", "path": str(tmp_path / "blob.bin")}]})
    gone = tools_for(tmp_path / "nothing.sock", tmp_path)
    with pytest.raises(BoardError, match="unreachable"):
        await gone.call("board_list", {})


async def test_first_turn_gets_the_briefing_and_tools_add_unread(
        board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    fake, socket = board
    fake.briefing = {**EMPTY_BRIEFING, "waiting_on_you": [THREAD]}
    h = Harness(tmp_path)
    h.bridge.board = tools_for(socket, tmp_path)
    await h.say(ALICE, "@me hi", mention=True)
    await h.bridge.one_turn()
    assert "<board>\nBoard briefing" in h.session.prompts[-1]
    assert "Threads waiting on you:\n- #7 Autosave spike" in h.session.prompts[-1]
    with pytest.raises(ToolError, match="thread 99 not found"):
        await h.bridge.tool_board("board_read", {"thread": 99})


async def test_an_unreachable_board_doesnt_stop_the_turn(tmp_path: Path) -> None:
    h = Harness(tmp_path)
    h.bridge.board = tools_for(tmp_path / "nothing.sock", tmp_path)
    await h.say(ALICE, "@me hi", mention=True)
    await h.bridge.one_turn()
    assert "(No board briefing: the board is unreachable" in h.session.prompts[-1]


async def test_schedules_name_their_thread(harness: Harness) -> None:
    await harness.bridge.tool_schedule({"note": "check the spike", "in_minutes": 5, "thread": 7})
    assert harness.bridge.schedules.items[0].note == "(board thread #7) check the spike"


def test_config_and_prompt(tmp_path: Path) -> None:
    config = make_config(tmp_path, board={"socket": "/run/agent-board/api.sock"})
    assert config.board == Board(socket=Path("/run/agent-board/api.sock"))
    assert "## The board" in system_prompt(config)
    assert "## The board" not in system_prompt(make_config(tmp_path))
    for bad in ({"url": "http://x"}, {"socket": "/s", "url": "http://x", "token": True}, {}):
        if bad:
            with pytest.raises(ConfigError):
                parse(config_data(board=bad), tmp_path)
    assert parse(config_data(board={}), tmp_path).board is None


def test_board_turns_auto_memory_off() -> None:
    base: dict[str, Any] = dict(workdir=Path("/w"), state=Path("/s"), cli_path=None, model=None, resume=None,
                                system_prompt="", permission_mode="default", allow=(), ask=(), deny=(),
                                token="t")
    assert "settings" not in options_kwargs(**base)
    assert '"autoMemoryEnabled": false' in options_kwargs(**base, auto_memory=False)["settings"]


__all__ = ["Bridge", "FakeChat", "FakeSession"]
