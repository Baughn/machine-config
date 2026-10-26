from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator
import logging
from pathlib import Path
from typing import Any

from aiohttp import web
import pytest

from agent_bridge import bridge as bridge_module
from agent_bridge.board import (BoardClient, BoardConfig, BoardError, BoardTools, age, briefing_text,
                                page_line, page_text, search_text, thread_line, thread_text, todo_line, unix)
from agent_bridge.bridge import Bridge
from agent_bridge.config import Board, ConfigError, parse
from agent_bridge.prompt import report_nudge, system_prompt, todos_line
from agent_bridge.render import Roots
from agent_bridge.session import BRIDGE_TOOLS, ToolError, TurnResult, options_kwargs
from conftest import ALICE, FakeChat, FakeSession, Harness, config_data, make_config

THREAD = {"id": 7, "title": "Autosave spike", "status": "open", "owner": "tsugumi-lab",
          "tags": ["perf"], "created": 1790946000, "updated": 1790949600, "due": "2026-10-04",
          "waiting_on": "baughn", "waiting_ref": "1555557513401860197",
          "summary": "Buffer the writes.\nNext: prototype.", "summary_author": "tsugumi-lab",
          "summary_updated": 1790949600, "summary_revisions": 2}
TODO = {"id": 3, "title": "fix X", "assignee": "tsugumi-lab", "created_by": "saya", "created": 1790946000,
        "due": "2026-10-25", "thread": 12, "status": "open", "closed_by": None, "closed": None, "note": None}
PAGE = {"slug": "mod/ea", "title": "Enchanting Apocalypse", "description": "Kits and their quirks",
        "owner": "tsugumi-minecraft", "tags": ["mod"], "rev": 3, "current_rev": 3, "created": 1790000000,
        "updated": 1790946000, "updated_by": "saya", "size": 17, "body": "Kits keep length.\n",
        "note": "found the cause", "sources": ["#35", "1564239183692959820"]}
EMPTY_BRIEFING = {"agent": "x", "since": None, "asks": [], "waiting_on_you": [],
                  "waiting_on_others": [], "due": [], "changed": []}


class FakeBoard:
    """The board's HTTP API on a unix socket, recording what it was asked."""

    def __init__(self) -> None:
        self.requests: list[tuple[str, str, dict[str, str], Any]] = []
        self.briefing: dict[str, Any] = EMPTY_BRIEFING
        self.todos: list[dict[str, Any]] = []
        self.todos_broken = False
        self.todos_delay = 0.0

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
        if path == "/todos" and request.method == "GET":
            if self.todos_broken:
                return web.json_response({"error": "database is locked"}, status=500)
            await asyncio.sleep(self.todos_delay)
            status = request.query.get("status", "open")
            return web.json_response({"todos": [t for t in self.todos if status in ("all", t["status"])]})
        if path == "/todos" and request.method == "POST":
            return web.json_response({**TODO, "id": 4, "assignee": "tsugumi-lab", "created_by": "tsugumi-lab",
                                      "due": None, "thread": None, **(body or {})})
        if path == "/todos/3" and request.method == "PATCH":
            closed = {"closed_by": "tsugumi-lab", "closed": 1790949600} if (body or {}).get("status") else {}
            return web.json_response({**TODO, **closed, **(body or {})})
        if path == "/search":
            return web.json_response([])
        if path == "/pages":
            return web.json_response({"pages": [PAGE] if request.query.get("prefix") != "none/" else []})
        if path == "/page" and request.method == "GET":
            if request.query.get("rev") == "1":
                return web.json_response({**PAGE, "rev": 1, "note": "", "sources": []})
            return web.json_response(PAGE)
        if path == "/page" and request.method == "PUT":
            if (body or {}).get("base_rev") == 2:
                return web.json_response({"error": "page mod/ea is at rev 3 (saya, 2026-10-02 13:00Z), not 2: "
                                                   "read it again and reapply your change"}, status=409)
            return web.json_response({**PAGE, "rev": 4, "current_rev": 4})
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


def test_todo_text() -> None:
    assert todo_line(TODO) == "#3 [open] fix X (assignee tsugumi-lab, from saya, due 2026-10-25, thread #12)"
    done = {**TODO, "created_by": "tsugumi-lab", "due": None, "thread": None, "status": "done",
            "closed_by": "tsugumi-lab", "closed": 1790949600, "note": "fixed in abc123"}
    assert todo_line(done) == ("#3 [done] fix X (assignee tsugumi-lab, closed by tsugumi-lab 2026-10-02 14:00Z)"
                               "\n    fixed in abc123")
    assert briefing_text({**EMPTY_BRIEFING, "todos": []}) == "Board: nothing is waiting for you."
    text = briefing_text({**EMPTY_BRIEFING, "todos": [TODO]})
    assert "Your open todos (close them with todo_update when done):\n- #3 [open] fix X" in text
    view = {"thread": {**THREAD, "summary": ""}, "posts": [], "todos": [TODO]}
    assert "Todos:\n- #3 [open] fix X" in thread_text(view)

    many = [{**TODO, "id": n, "title": f"item {n}"} for n in range(1, 8)]
    assert todos_line([]) is None
    assert todos_line(many) == ("Your open todos (board): #1 item 1; #2 item 2; #3 item 3; #4 item 4; "
                                "#5 item 5 (and 2 more). Close them with todo_update when done.")
    assert report_nudge([]) == ""
    assert report_nudge(many[:1]) == ("\nYou still have 1 open todo: #1 item 1 — close what's finished "
                                      "(todo_update) or say in the post what's left.")


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


async def test_todo_tools(board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    fake, socket = board
    tools = tools_for(socket, tmp_path)
    assert "mcp__bridge__todo_update" in BRIDGE_TOOLS
    added = await tools.call("todo_add", {"title": "check the fix", "assignee": "baughn", "thread": "12"})
    assert added == "added: #4 [open] check the fix (assignee baughn, from tsugumi-lab, thread #12)"
    assert fake.requests[-1][1:] == ("/todos", {}, {"title": "check the fix", "assignee": "baughn", "thread": 12})

    fake.todos = [TODO, {**TODO, "id": 5, "status": "done"}]
    assert await tools.call("todo_list", {}) == todo_line(TODO)
    assert fake.requests[-1][2] == {"assignee": "tsugumi-lab"}
    listed = await tools.call("todo_list", {"assignee": "all", "status": "all", "thread": 12})
    assert listed.count("\n") == 1 and fake.requests[-1][2] == {"status": "all", "thread": "12"}
    assert await tools.call("todo_list", {"status": "dropped"}) == "no todos"

    updated = await tools.call("todo_update", {"id": 3, "status": "done", "note": "fixed", "thread": None})
    assert updated.startswith("updated: #3 [done] fix X (assignee tsugumi-lab, from saya, due 2026-10-25, "
                              "closed by tsugumi-lab")
    assert fake.requests[-1][3] == {"status": "done", "note": "fixed", "thread": None}
    with pytest.raises(BoardError, match="nothing to change"):
        await tools.call("todo_update", {"id": 3})


def test_wiki_text() -> None:
    now = 1790946000 + 3 * 3600 + 120
    assert age(1790946000, now) == "3h ago" and age(now, now) == "just now" and age(None) == "?"
    assert page_line(PAGE, now) == ("mod/ea (rev 3, tsugumi-minecraft, updated 3h ago by saya): "
                                    "Enchanting Apocalypse - Kits and their quirks")
    assert page_line({**PAGE, "description": ""}, now).endswith("by saya): Enchanting Apocalypse")
    assert page_text(PAGE) == (
        "slug: mod/ea\ntitle: Enchanting Apocalypse\ndescription: Kits and their quirks\n"
        "owner: tsugumi-minecraft; tags: mod\nrev 3 of 3\nlast change: saya, 2026-10-02 13:00Z\n"
        "note (rev 3): found the cause\nsources: #35, 1564239183692959820\n\nKits keep length.\n\n"
        "To change it: wiki_write with base_rev 3.")
    old = page_text({**PAGE, "rev": 1})
    assert "rev 1 of 3 (an old revision)" in old and old.endswith("base_rev 3.")
    hit = {"kind": "page", "id": 2, "slug": "mod/ea", "thread": None, "thread_title": "Enchanting Apocalypse",
           "author": "saya", "created": 1790946000, "snippet": "Kits keep", "superseded": False}
    assert search_text([hit]) == "page mod/ea 'Enchanting Apocalypse' by saya, 2026-10-02 13:00Z: Kits keep"


async def test_wiki_tools(board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    fake, socket = board
    tools = tools_for(socket, tmp_path)
    assert {"mcp__bridge__wiki_list", "mcp__bridge__wiki_read", "mcp__bridge__wiki_write"} <= set(BRIDGE_TOOLS)
    assert (await tools.call("wiki_list", {"prefix": "mod/"})).startswith("mod/ea (rev 3, tsugumi-minecraft")
    assert fake.requests[-1][1:3] == ("/pages", {"prefix": "mod/"})
    assert await tools.call("wiki_list", {"prefix": "none/"}) == "no pages"
    assert "rev 1 of 3" in await tools.call("wiki_read", {"slug": "mod/ea", "rev": 1})
    assert fake.requests[-1][1:3] == ("/page", {"slug": "mod/ea", "rev": "1"})
    await tools.call("board_search", {"query": "kits", "kind": "page"})
    assert fake.requests[-1][2] == {"q": "kits", "kind": "page"}

    result = await tools.call("wiki_write", {"slug": "mod/ea", "body": "New.", "base_rev": 3, "note": "why",
                                             "sources": [35], "tags": ["mod"]})
    assert result == "wrote mod/ea rev 4\nhttps://agents.brage.info/w/mod/ea"
    assert fake.requests[-1][:2] == ("PUT", "/page")
    assert fake.requests[-1][3] == {"slug": "mod/ea", "body": "New.", "note": "why", "base_rev": 3,
                                    "sources": ["35"], "tags": ["mod"]}
    (tmp_path / "page.md").write_text("From a file.")
    await tools.call("wiki_write", {"slug": "proc/x", "path": str(tmp_path / "page.md"), "note": "new",
                                    "title": "X"})
    assert fake.requests[-1][3] == {"slug": "proc/x", "body": "From a file.", "note": "new", "title": "X"}

    with pytest.raises(BoardError, match="at rev 3 .* read it again"):
        await tools.call("wiki_write", {"slug": "mod/ea", "body": "x", "base_rev": 2, "note": "n"})
    with pytest.raises(BoardError, match="exactly one of body and path"):
        await tools.call("wiki_write", {"slug": "mod/ea", "note": "n"})
    with pytest.raises(BoardError, match="note is required"):
        await tools.call("wiki_write", {"slug": "mod/ea", "body": "x", "note": " "})
    with pytest.raises(BoardError, match="outside the workdir"):
        await tools.call("wiki_write", {"slug": "mod/ea", "path": "/etc/passwd", "note": "n"})
    (tmp_path / "blob.bin").write_bytes(b"\xff\xfe\x00")
    with pytest.raises(BoardError, match="isn't UTF-8"):
        await tools.call("wiki_write", {"slug": "mod/ea", "path": str(tmp_path / "blob.bin"), "note": "n"})


async def test_a_wiki_conflict_is_a_tool_error(board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    _, socket = board
    h = Harness(tmp_path)
    h.bridge.board = tools_for(socket, tmp_path)
    with pytest.raises(ToolError, match="board: page mod/ea is at rev 3 .* not 2: read it again"):
        await h.bridge.tool_board("wiki_write", {"slug": "mod/ea", "body": "x", "base_rev": 2, "note": "n"})


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


async def test_each_turn_lists_open_todos(board: tuple[FakeBoard, Path], tmp_path: Path,
                                          monkeypatch: pytest.MonkeyPatch,
                                          caplog: pytest.LogCaptureFixture) -> None:
    fake, socket = board
    h = Harness(tmp_path)
    h.bridge.board = tools_for(socket, tmp_path)

    async def turn() -> str:
        await h.say(ALICE, "@me go", mention=True)
        await h.bridge.one_turn()
        return h.session.prompts[-1]

    assert "open todos" not in await turn()
    fake.todos = [{**TODO, "id": n, "title": f"item {n}"} for n in range(1, 8)]
    assert ("\n\nYour open todos (board): #1 item 1; #2 item 2; #3 item 3; #4 item 4; #5 item 5 "
            "(and 2 more). Close them with todo_update when done.") in await turn()
    assert ("GET", "/todos", {"assignee": "tsugumi-lab", "status": "open"}, None) in fake.requests

    fake.todos_broken = True
    with caplog.at_level(logging.WARNING, logger="agent_bridge.bridge"):
        assert "open todos" not in await turn()
        assert "open todos" not in await turn()
    assert sum("open todos" in r.message for r in caplog.records) == 1
    fake.todos_broken = False
    fake.todos_delay = 1.0
    monkeypatch.setattr(bridge_module, "TODO_TIMEOUT", 0.05)
    assert "open todos" not in await turn()
    fake.todos_delay = 0.0
    assert "Your open todos (board): #1 item 1" in await turn()


async def test_a_report_mentions_open_todos(board: tuple[FakeBoard, Path], tmp_path: Path) -> None:
    fake, socket = board
    h = Harness(tmp_path)
    h.bridge.board = tools_for(socket, tmp_path)
    results: list[str] = []

    async def script(session: FakeSession, prompt: str) -> TurnResult:
        results.append(await session.bridge.tool_post({"kind": "report", "headline": "All done"}))
        results.append(await session.bridge.tool_post({"kind": "status", "headline": "Still here"}))
        return TurnResult("session-1")

    h.session.script = script
    fake.todos = [TODO, {**TODO, "id": 5, "title": "write it up"}]
    await h.say(ALICE, "@me go", mention=True)
    await h.bridge.one_turn()
    assert results[0].endswith("\nYou still have 2 open todos: #3 fix X, #5 write it up — close what's "
                               "finished (todo_update) or say in the post what's left.\nunread: 0")
    assert "open todo" not in results[1]

    for broken in (False, True):
        fake.todos, fake.todos_broken = ([], False) if not broken else ([TODO], True)
        await h.say(ALICE, "@me again", mention=True)
        await h.bridge.one_turn()
        assert "open todo" not in results[-2] and results[-2].endswith("unread: 0")


async def test_schedules_name_their_thread(harness: Harness) -> None:
    await harness.bridge.tool_schedule({"note": "check the spike", "in_minutes": 5, "thread": 7})
    assert harness.bridge.schedules.items[0].note == "(board thread #7) check the spike"


def test_config_and_prompt(tmp_path: Path) -> None:
    config = make_config(tmp_path, board={"socket": "/run/agent-board/api.sock"})
    assert config.board == Board(socket=Path("/run/agent-board/api.sock"))
    assert "## The board" in system_prompt(config)
    assert "The wiki (`wiki_*` tools; https://agents.brage.info/w)" in system_prompt(config)
    assert "tagged `reference`" not in system_prompt(config)
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
