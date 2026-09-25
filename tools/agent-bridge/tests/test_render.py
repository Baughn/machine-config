from __future__ import annotations

from pathlib import Path

import pytest

from agent_bridge.filter import find_secret
from agent_bridge.limits import Breaker, Streak
from agent_bridge.config import Limits
from agent_bridge.policy import Kind
from agent_bridge.render import (HEADLINE_LIMIT, MESSAGE_LIMIT, OVERVIEW_LIMIT, PostError, Roots, Status,
                                 approval_request, render_post, summarize_tool)


def roots(tmp: Path) -> Roots:
    return Roots((tmp / "work", tmp / "state"), 1000)


def post(tmp: Path, **args: object) -> object:
    return render_post({"kind": "report", "headline": "h", **args}, owner_id="1", roots=roots(tmp))


def test_limits_exactly_at_the_boundaries(tmp_path: Path) -> None:
    post(tmp_path, headline="x" * HEADLINE_LIMIT, overview="y" * OVERVIEW_LIMIT)
    with pytest.raises(PostError, match="headline"):
        post(tmp_path, headline="x" * (HEADLINE_LIMIT + 1))
    with pytest.raises(PostError, match="attachment"):
        post(tmp_path, overview="y" * (OVERVIEW_LIMIT + 1))
    with pytest.raises(PostError):
        post(tmp_path, headline="two\nlines")


def test_kinds(tmp_path: Path) -> None:
    with pytest.raises(PostError, match="kind"):
        post(tmp_path, kind="shout")
    with pytest.raises(PostError, match="plan needs"):
        post(tmp_path, kind="plan")
    plan = render_post({"kind": "plan", "headline": "h", "attachments": [{"name": "plan.md", "content": "x"}]},
                       owner_id="1", roots=roots(tmp_path))
    assert plan.files[0].name == "plan.md"  # type: ignore[attr-defined]
    alert = render_post({"kind": "alert", "headline": "h"}, owner_id="1", roots=roots(tmp_path))
    assert alert.content.startswith("<@1> ") and alert.mention_users == ("1",)
    report = render_post({"kind": "report", "headline": "h"}, owner_id="1", roots=roots(tmp_path))
    assert "<@" not in report.content and report.mention_users == ()


def test_path_attachments(tmp_path: Path) -> None:
    (tmp_path / "work").mkdir()
    (tmp_path / "work/log.txt").write_text("log")
    (tmp_path / "work/big.txt").write_text("x" * 1001)
    (tmp_path / "secret.txt").write_text("nope")
    (tmp_path / "work/escape").symlink_to(tmp_path / "secret.txt")
    ok = post(tmp_path, attachments=[{"name": "log.txt", "path": str(tmp_path / "work/log.txt")}])
    assert ok.files[0].data == b"log"  # type: ignore[attr-defined]
    for path in (tmp_path / "secret.txt", tmp_path / "work/escape", tmp_path / "work/../secret.txt"):
        with pytest.raises(PostError, match="outside"):
            post(tmp_path, attachments=[{"name": "x.txt", "path": str(path)}])
    with pytest.raises(PostError, match="larger"):
        post(tmp_path, attachments=[{"name": "big.txt", "path": str(tmp_path / "work/big.txt")}])
    for name in ("../x", "a/b", ".hidden", ""):
        with pytest.raises(PostError):
            post(tmp_path, attachments=[{"name": name, "content": "x"}])
    with pytest.raises(PostError, match="at most"):
        post(tmp_path, attachments=[{"name": f"f{i}", "content": "x"} for i in range(11)])


def test_secret_filter(tmp_path: Path) -> None:
    with pytest.raises(PostError, match="private key"):
        post(tmp_path, overview="-----BEGIN OPENSSH PRIVATE KEY-----\nabc")
    with pytest.raises(PostError, match="RCON"):
        post(tmp_path, attachments=[{"name": "server.properties", "content": "rcon.password=hunter2\n"}])
    assert find_secret("AGE-SECRET-KEY-1QQQQQQQQQQQQQQQQQQQQQQQQQQ") == "age secret key"
    assert find_secret("token tok-12345678 here", ("tok-12345678",)) is not None
    assert find_secret("rcon.password=") is None
    assert find_secret("the private key lives in agenix") is None
    assert find_secret("sk-ant-oat01-" + "a" * 30) == "Anthropic token"


def test_status_message() -> None:
    status = Status("tsugumi-minecraft", "alice", 0.0, tools=12, last="Bash `zfs list`", pending=1)
    text = status.render(42.0)
    assert text.splitlines()[0] == "⚙ tsugumi-minecraft · working for alice · 00:42"
    assert "12 tool calls · 1 approval pending" in text
    assert status.render(42.0, "silent").startswith("💤")
    status.last = "x" * 5000
    assert len(status.render(1.0)) <= MESSAGE_LIMIT


def test_summaries_and_approval_requests() -> None:
    assert summarize_tool("Bash", {"command": "ls\n-la"}) == "Bash `ls -la`"
    assert summarize_tool("mcp__bridge__post", {"kind": "x"}) == "post"
    assert len(summarize_tool("Bash", {"command": "x" * 500})) < 140
    short = approval_request("id", "Bash", {"command": "ls"}, "because")
    assert "```json" in short.content and not short.files
    long = approval_request("id", "Write", {"content": "x" * 3000}, None)
    assert long.files and len(long.content) <= MESSAGE_LIMIT


def test_streak() -> None:
    streak = Streak.from_history([Kind.HUMAN, Kind.AGENT, Kind.SELF, Kind.WATCHDOG, Kind.AGENT])
    assert streak.count == 3
    streak.observe(Kind.HUMAN)
    assert streak.count == 0


def test_breaker_trips_once_and_stays_tripped() -> None:
    breaker = Breaker(Limits(posts_per_minute=2, posts_per_hour=3, turns_per_hour=2))
    assert breaker.post(0) and breaker.post(1)
    assert not breaker.post(2)
    assert breaker.tripped == "more than 2 posts per minute"
    assert not breaker.post(3600 * 5)  # stays tripped until reset
    breaker.reset()
    assert breaker.post(100) and breaker.post(200) and breaker.post(300)
    assert not breaker.post(400)
    assert breaker.tripped == "more than 3 posts per hour"
    breaker.reset()
    assert breaker.turn(0) and breaker.turn(1) and not breaker.turn(2)
    breaker.reset()
    assert breaker.turn(0) and breaker.turn(1) and breaker.post(5000)
    assert breaker.turn(3601)
