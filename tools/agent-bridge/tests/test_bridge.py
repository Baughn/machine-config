"""The real bridge against a fake chat and a scripted fake session."""

from __future__ import annotations

import asyncio
import itertools
import json
from pathlib import Path
import re
from typing import Any

import pytest

from agent_bridge.approval import STOP, Verdict
from agent_bridge.bridge import Bridge
from agent_bridge.config import ConfigError, parse
from agent_bridge.policy import Attachment, Incoming, Route, classify, may_approve, route
from agent_bridge.render import Outgoing
from agent_bridge.session import Permission, ToolError, TurnResult

from conftest import (ALICE, CAROL, CHANNEL, LAB, ME, OWNER, FakeChat, FakeSession, Harness, config_data,
                      message)


async def run_turn(h: Harness) -> None:
    await h.bridge.one_turn()


def status_id(h: Harness) -> str:
    return next(i for i, m in h.chat.sent.items() if "working for" in m.content)


async def settle() -> None:
    for _ in range(20):
        await asyncio.sleep(0)


# --- turns -----------------------------------------------------------------


async def test_a_turn_that_posts(harness: Harness) -> None:
    async def script(session: FakeSession, prompt: str) -> TurnResult:
        result = await session.bridge.tool_post({"kind": "report", "headline": "All good"})
        assert result.endswith("unread: 0")
        return TurnResult("session-1")

    harness.session.script = script
    trigger = await harness.say(ALICE, "@me how are the worlds?", mention=True)
    await run_turn(harness)
    assert f"[{trigger.id}] alice (human, admin), may ask you to act: @me how are the worlds?" in harness.session.prompts[0]
    sid = status_id(harness)
    assert harness.chat.sent[sid].reply_to == trigger.id
    assert harness.chat.final(sid).startswith("✓ tsugumi-minecraft · working for alice")
    assert "📄 **All good**" in harness.chat.texts()
    assert json.loads((harness.config.state / "session.json").read_text())["session_id"] == "session-1"
    assert len(list((harness.config.state / "turns").glob("*.log"))) == 1


async def test_a_silent_turn_posts_nothing_else(harness: Harness) -> None:
    await harness.say(OWNER, "fyi", mention=True)
    await run_turn(harness)
    assert len(harness.chat.sent) == 1
    assert harness.chat.final(status_id(harness)).startswith("💤")


async def test_errors(harness: Harness) -> None:
    async def failing(session: FakeSession, prompt: str) -> TurnResult:
        return TurnResult("session-1", error="api overloaded")

    async def raising(session: FakeSession, prompt: str) -> TurnResult:
        raise RuntimeError("cli died")

    for script in (failing, raising):
        harness.chat.sent.clear()
        harness.session.script = script
        await harness.say(OWNER, "go", mention=True)
        await run_turn(harness)
        assert harness.chat.final(status_id(harness)).startswith("✗")


async def test_context_is_delivered_with_the_next_turn(harness: Harness) -> None:
    await harness.say(CAROL, "chatter")  # admin, not a mention: context
    await harness.say(LAB, "lab here")  # agent, not a mention: context
    await harness.say(CAROL, "nobody listens", admin=False)  # no admin role: ignored
    assert not harness.bridge.wake.is_set()
    await harness.say(ALICE, "@me go", mention=True)
    await run_turn(harness)
    prompt = harness.session.prompts[0]
    assert "carol (human, admin), context only: chatter" in prompt
    assert "lab (agent, agent), context only: lab here" in prompt
    assert "nobody listens" not in prompt


async def test_messages_arriving_mid_turn(harness: Harness) -> None:
    arrived = asyncio.Event()

    async def script(session: FakeSession, prompt: str) -> TurnResult:
        await arrived.wait()
        result = await session.bridge.tool_post({"kind": "status", "headline": "working"})
        assert result.endswith("unread: 2")
        inbox = await session.bridge.tool_inbox({})
        assert "also this" in inbox and inbox.endswith("unread: 0")
        return TurnResult("session-1")

    harness.session.script = script
    await harness.say(ALICE, "@me first", mention=True)
    turn = asyncio.create_task(run_turn(harness))
    await settle()
    await harness.say(ALICE, "@me also this", mention=True)
    await harness.say(CAROL, "and chatter")
    arrived.set()
    await turn
    # inbox delivered the trigger, so no follow-up turn is due.
    assert not any(e.trigger for e in harness.bridge.buffer)


async def test_unread_trigger_starts_a_follow_up_turn(harness: Harness) -> None:
    arrived = asyncio.Event()

    async def script(session: FakeSession, prompt: str) -> TurnResult:
        if len(session.prompts) == 1:
            await arrived.wait()
        return TurnResult("session-1")

    harness.session.script = script
    runner = asyncio.create_task(harness.bridge.run())
    await harness.say(ALICE, "@me first", mention=True)
    await settle()
    await harness.say(ALICE, "@me second", mention=True)
    arrived.set()
    await settle()
    runner.cancel()
    assert len(harness.session.prompts) == 2
    assert "@me second" in harness.session.prompts[1] and "@me first" not in harness.session.prompts[1]


async def test_attachments_from_humans_are_downloaded(harness: Harness) -> None:
    attachment = Attachment("crash.log", "https://cdn/crash.log", 10)
    big = Attachment("huge.bin", "https://cdn/huge.bin", 10**9)
    trigger = await harness.say(ALICE, "@me look", mention=True, attachments=(attachment, big))
    await run_turn(harness)
    path = harness.config.state / "inbox" / f"{trigger.id}-crash.log"
    assert path.read_text() == "downloaded https://cdn/crash.log"
    assert str(path) in harness.session.prompts[0]
    assert "huge.bin (not downloaded" in harness.session.prompts[0]


async def test_post_errors_go_back_to_the_agent(harness: Harness) -> None:
    with pytest.raises(ToolError, match="attachment"):
        await harness.bridge.tool_post({"kind": "report", "headline": "h", "overview": "x" * 2000})
    with pytest.raises(ToolError, match="secret"):
        await harness.bridge.tool_post({"kind": "report", "headline": "tok-discord-secret"})
    assert harness.chat.sent == {}


# --- approvals ---------------------------------------------------------------


class Asker:
    """A script that requests one permission and records the answer."""

    def __init__(self, name: str = "Bash", tool_input: dict[str, Any] | None = None) -> None:
        self.name = name
        self.input = tool_input or {"command": "systemctl restart minecraft@erisia"}
        self.result: Permission | None = None
        self.asked = asyncio.Event()

    async def __call__(self, session: FakeSession, prompt: str) -> TurnResult:
        task = asyncio.create_task(session.bridge.permission(self.name, self.input, "restart to apply"))
        await settle()
        self.asked.set()
        self.result = await task
        return TurnResult("session-1")


async def start_approval(h: Harness, asker: Asker) -> tuple[asyncio.Task[None], str]:
    h.session.script = asker
    await h.say(ALICE, "@me restart erisia", mention=True)
    turn = asyncio.create_task(run_turn(h))
    await asker.asked.wait()
    request = next(i for i, m in h.chat.sent.items() if "asks" in m.content)
    return turn, request


async def test_approval_allowed(harness: Harness) -> None:
    asker = Asker()
    turn, request = await start_approval(harness, asker)
    assert harness.chat.sent[request].reply_to == status_id(harness)
    assert request in harness.chat.approvals
    assert "1 approval pending" in harness.bridge.status.render(0)  # type: ignore[union-attr]
    # Bots, non-approvers and humans without the role are ignored.
    for reactor in (harness.author(LAB), harness.author(ME), harness.author(CAROL, admin=False)):
        assert await harness.bridge.on_decide(request, reactor, Verdict.ALLOW) == "Only approvers can decide."
    await settle()
    assert asker.result is None
    assert await harness.bridge.on_decide(request, harness.author(ALICE), Verdict.ALLOW) == "Allowed."
    await turn
    assert asker.result == Permission(True)
    assert harness.chat.final(request).endswith("✅ approved by alice")


async def test_approval_denied(harness: Harness) -> None:
    asker = Asker()
    turn, request = await start_approval(harness, asker)
    await harness.bridge.on_decide(request, harness.author(OWNER), Verdict.DENY)
    await turn
    assert asker.result == Permission(False, "Denied by baughn.")


async def test_approval_times_out(tmp_path: Path) -> None:
    h = Harness(tmp_path, approval_timeout=0.05)
    asker = Asker()
    turn, request = await start_approval(h, asker)
    await turn
    assert asker.result is not None and not asker.result.allow
    assert "No approver answered" in asker.result.message


async def test_stop_while_an_approval_is_pending(harness: Harness) -> None:
    asker = Asker()
    turn, request = await start_approval(harness, asker)
    await harness.say(ALICE, "!stop tsugumi-minecraft")
    await turn
    assert asker.result is not None and not asker.result.allow and "stopped by alice" in asker.result.message
    assert harness.session.interrupted.is_set()
    assert harness.chat.final(status_id(harness)).startswith("⏹")


async def test_stop_command_from_a_non_approver_is_ignored(harness: Harness) -> None:
    asker = Asker()
    turn, request = await start_approval(harness, asker)
    await harness.say(CAROL, "!stop all", admin=False)
    await harness.say(LAB, "!stop all")
    await settle()
    assert asker.result is None
    await harness.bridge.on_decide(request, harness.author(ALICE), Verdict.ALLOW)
    await turn


QUESTIONS = [
    {"question": "Restart now?", "header": "Restart", "multiSelect": False,
     "options": [{"label": "Yes"}, {"label": "Tonight"}]},
    {"question": "Which worlds?", "header": "Worlds", "multiSelect": True,
     "options": [{"label": "erisia"}, {"label": "incognito"}]},
]


async def test_ask_user_question(harness: Harness) -> None:
    asker = Asker("AskUserQuestion", {"questions": QUESTIONS})
    turn, request = await start_approval(harness, asker)
    assert harness.chat.questions[request] == QUESTIONS
    assert await harness.bridge.on_select(request, harness.author(CAROL, admin=False), 0, ["Yes"]) == "Only approvers can answer."
    assert await harness.bridge.on_select(request, harness.author(LAB), 0, ["Yes"]) == "Only approvers can answer."
    assert "remaining" in await harness.bridge.on_select(request, harness.author(ALICE), 0, ["Tonight"])
    # Allow/Deny buttons don't answer questions.
    assert "no longer open" in await harness.bridge.on_decide(request, harness.author(ALICE), Verdict.ALLOW)
    assert await harness.bridge.on_select(request, harness.author(OWNER), 1, ["erisia", "incognito"]) == "Answer sent."
    await turn
    assert asker.result is not None and asker.result.allow
    assert asker.result.updated_input == {"questions": QUESTIONS, "answers": {
        "Restart now?": "Tonight", "Which worlds?": ["erisia", "incognito"]}}
    # The channel records who chose what.
    assert harness.chat.final(request).endswith(
        "✅ answered:\n**1.** → Tonight (alice)\n**2.** → erisia, incognito (baughn)")


async def test_ask_user_question_times_out(tmp_path: Path) -> None:
    h = Harness(tmp_path, approval_timeout=0.05)
    asker = Asker("AskUserQuestion", {"questions": QUESTIONS})
    turn, _ = await start_approval(h, asker)
    await turn
    assert asker.result is not None and not asker.result.allow


async def test_malformed_question_is_denied(harness: Harness) -> None:
    result = await harness.bridge.permission("AskUserQuestion", {"questions": []}, None)
    assert not result.allow


# --- stop, commands, limits ---------------------------------------------------


async def interruptible(session: FakeSession, prompt: str) -> TurnResult:
    await session.bridge.tool_started("Bash", {"command": "sleep 600"})
    await session.interrupted.wait()
    return TurnResult("session-1", interrupted=True)


async def test_stop_reaction_on_our_message(harness: Harness) -> None:
    harness.session.script = interruptible
    await harness.say(ALICE, "@me sleep", mention=True)
    turn = asyncio.create_task(run_turn(harness))
    await settle()
    assert "Bash `sleep 600`" in harness.bridge.status.render(0)  # type: ignore[union-attr]
    await harness.bridge.on_reaction(status_id(harness), True, harness.author(LAB), STOP)
    await harness.bridge.on_reaction(status_id(harness), False, harness.author(ALICE), STOP)
    await settle()
    assert not turn.done()
    await harness.bridge.on_reaction(status_id(harness), True, harness.author(ALICE), STOP)
    await turn
    assert harness.chat.final(status_id(harness)).startswith("⏹")
    # The session continues.
    harness.session.script = lambda s, p: asyncio.sleep(0, TurnResult("session-1"))
    await harness.say(ALICE, "@me again", mention=True)
    await run_turn(harness)
    assert len(harness.session.prompts) == 2


async def test_pause_resume_and_status(harness: Harness) -> None:
    await harness.say(OWNER, "!pause tsugumi-minecraft")
    await harness.say(ALICE, "@me work", mention=True)
    assert not harness.bridge.wake.is_set()
    await harness.say(ALICE, "!status")
    assert "paused" in harness.chat.texts()[-1]
    await harness.say(ALICE, "!resume all")
    assert harness.bridge.wake.is_set()
    # Commands for other identities are not ours.
    await harness.say(ALICE, "!pause tsugumi-lab")
    assert not harness.bridge.paused


async def test_reset_starts_a_new_session(harness: Harness) -> None:
    await harness.say(ALICE, "@me hi", mention=True)
    await run_turn(harness)
    await harness.say(OWNER, "!reset tsugumi-minecraft")
    assert harness.session.connects[-1] is None
    assert json.loads((harness.config.state / "session.json").read_text()) == {"session_id": None, "last_turn": None}


async def test_status_log(harness: Harness) -> None:
    async def script(session: FakeSession, prompt: str) -> TurnResult:
        await session.bridge.tool_started("Read", {"file_path": "/srv/x"})
        await session.bridge.tool_finished("Read", False)
        return TurnResult("session-1")

    harness.session.script = script
    await harness.say(ALICE, "@me read", mention=True)
    await run_turn(harness)
    await harness.say(ALICE, "!status tsugumi-minecraft log")
    log = list(harness.chat.sent.values())[-1]
    assert b"Read" in log.files[0].data


async def test_circuit_breaker(tmp_path: Path) -> None:
    h = Harness(tmp_path, limits={"posts_per_minute": 2})

    async def chatty(session: FakeSession, prompt: str) -> TurnResult:
        for _ in range(5):
            try:
                await session.bridge.tool_post({"kind": "status", "headline": "spam"})
            except ToolError:
                pass
        return TurnResult("session-1")

    h.session.script = chatty
    await h.say(ALICE, "@me go", mention=True)
    await run_turn(h)
    texts = h.chat.texts()
    assert texts.count("💬 **spam**") == 2
    notices = [t for t in texts if "circuit breaker" in t]
    assert len(notices) == 1 and notices[0].startswith(f"<@{OWNER}>")
    h.bridge.wake.clear()
    await h.say(ALICE, "@me more", mention=True)
    assert not h.bridge.wake.is_set()
    await h.say(ALICE, "!resume tsugumi-minecraft")
    assert h.bridge.breaker.tripped is None and h.bridge.wake.is_set()


async def test_restart_resumes_the_session(tmp_path: Path) -> None:
    first = Harness(tmp_path)
    await first.say(ALICE, "@me hi", mention=True)
    await run_turn(first)
    second = Harness(tmp_path)
    await second.bridge.start()
    assert second.session.connects == ["session-1"]
    third = Harness(tmp_path)
    third.session.fail_resume = True
    await third.bridge.start()
    assert third.session.connects == ["session-1", None]


# --- two agents ---------------------------------------------------------------


class Relay:
    """A channel shared by two bridges: each post reaches both, from its agent."""

    def __init__(self) -> None:
        self.bridges: list[Bridge] = []
        self.posts = 0
        self.counter = 0

    def chat(self, sender_id: str) -> FakeChat:
        relay = self

        class RelayChat(FakeChat):
            async def send(self, message: Outgoing) -> str:
                message_id = await super().send(message)
                if "**" in message.content:  # a post, not a status message
                    relay.posts += 1
                    other = LAB if sender_id == ME else ME
                    for bridge in relay.bridges:
                        author = bridge_author(bridge, sender_id)
                        await bridge.on_message(Incoming(message_id + sender_id, CHANNEL, None, author,
                                                         message.content, mentions=frozenset({other})))
                return message_id

        return RelayChat()


def bridge_author(bridge: Bridge, discord_id: str) -> Any:
    return classify(bridge.config, discord_id, "x", is_bot=True, webhook_id=None, is_admin=False)


async def test_two_agents_are_stopped_by_the_streak_breaker(tmp_path: Path) -> None:
    relay = Relay()

    async def reply(session: FakeSession, prompt: str) -> TurnResult:
        await session.bridge.tool_post({"kind": "status", "headline": "your turn"})
        return TurnResult("s")

    for identity, discord_id in (("tsugumi-minecraft", ME), ("tsugumi-lab", LAB)):
        (tmp_path / identity / "work").mkdir(parents=True)
        (tmp_path / identity / "state").mkdir()
        data = config_data(id=identity, workdir=str(tmp_path / identity / "work"), limits={"bot_streak": 6})
        config = parse(data, tmp_path / identity / "state")
        bridge = Bridge(config, relay.chat(discord_id), FakeSession)
        assert isinstance(bridge.session, FakeSession)
        bridge.session.script = reply
        relay.bridges.append(bridge)
    runners = [asyncio.create_task(b.run()) for b in relay.bridges]
    alice = classify(relay.bridges[0].config, ALICE, "alice", is_bot=False, webhook_id=None, is_admin=True)
    human = Incoming("h1", CHANNEL, None, alice, "@me talk", mentions=frozenset({ME}))
    for bridge in relay.bridges:
        await bridge.on_message(human)
    for _ in range(50):
        await settle()
    for runner in runners:
        runner.cancel()
    assert 6 <= relay.posts <= 8, relay.posts


# --- failures -------------------------------------------------------------------


async def test_startup_failure_is_not_swallowed(tmp_path: Path) -> None:
    h = Harness(tmp_path)

    async def broken(resume: str | None) -> None:
        raise RuntimeError("claude not found")

    h.session.connect = broken  # type: ignore[method-assign]
    with pytest.raises(RuntimeError, match="claude not found"):
        await h.bridge.start()


async def test_a_crashed_cli_is_reconnected(harness: Harness) -> None:
    async def crash(session: FakeSession, prompt: str) -> TurnResult:
        raise ConnectionError("CLI exited")

    await harness.bridge.start()
    harness.session.script = crash
    await harness.say(ALICE, "@me go", mention=True)
    await run_turn(harness)
    assert harness.session.disconnects == 1
    assert harness.session.connects == [None, None]
    assert harness.chat.final(status_id(harness)).startswith("✗")


async def test_a_failed_status_post_does_not_kill_the_loop(harness: Harness) -> None:
    calls = 0
    send = harness.chat.send

    async def flaky(message: Outgoing) -> str:
        nonlocal calls
        calls += 1
        if calls == 1:
            raise ConnectionError("discord hiccup")
        return await send(message)

    harness.chat.send = flaky  # type: ignore[method-assign]
    runner = asyncio.create_task(harness.bridge.run())
    await harness.say(ALICE, "@me one", mention=True)
    await settle()
    await harness.say(ALICE, "@me two", mention=True)
    await settle()
    assert not runner.done()
    runner.cancel()
    # Nothing was lost: the retry carries both messages.
    assert len(harness.session.prompts) == 1
    assert "@me one" in harness.session.prompts[0] and "@me two" in harness.session.prompts[0]


# --- session rollover ------------------------------------------------------------

HOUR = 3600.0


async def handoff_turn(h: Harness) -> None:
    await h.say(ALICE, "@me hi", mention=True)
    await run_turn(h)


def write_handoff(h: Harness, text: str) -> None:
    (h.config.workdir / "notes").mkdir(parents=True, exist_ok=True)
    (h.config.workdir / "notes/handoff.md").write_text(text)


async def test_idle_session_writes_a_handoff_then_starts_afresh(harness: Harness) -> None:
    await handoff_turn(harness)
    harness.clock.now += 5 * HOUR
    await harness.bridge.maybe_rollover()
    assert len(harness.session.prompts) == 1

    async def handoff(session: FakeSession, prompt: str) -> TurnResult:
        assert "handoff.md" in prompt
        for call in (session.bridge.tool_post({"kind": "report", "headline": "bye"}),
                     session.bridge.tool_inbox({}),
                     session.bridge.tool_rcon({"world": "erisia", "command": "list"})):
            with pytest.raises(ToolError, match="handoff"):
                await call
        denied = await session.bridge.permission("Bash", {"command": "rm -rf /"}, None)
        assert not denied.allow
        write_handoff(harness, "check the 06:00 restart")
        return TurnResult("session-1")

    harness.session.script = handoff
    harness.clock.now += 1.01 * HOUR
    await harness.say(CAROL, "chatter meanwhile")
    await harness.bridge.maybe_rollover()
    assert len(harness.session.prompts) == 2
    assert harness.session.connects[-1] is None
    assert harness.bridge.session_id is None
    assert len(harness.chat.sent) == 1  # only the first turn's status message
    assert len(harness.bridge.buffer) == 1  # the chatter waits for the new session
    assert any("handoff" in path.read_text() for path in (harness.config.state / "turns").glob("*.log"))

    harness.session.script = silent_script
    await harness.say(ALICE, "@me again", mention=True)
    await run_turn(harness)
    first = harness.session.prompts[-1]
    assert "new session" in first and "check the 06:00 restart" in first and "chatter meanwhile" in first
    await harness.say(ALICE, "@me and again", mention=True)
    await run_turn(harness)
    assert "new session" not in harness.session.prompts[-1]


async def silent_script(session: FakeSession, prompt: str) -> TurnResult:
    return TurnResult("session-2")


async def test_no_rollover_without_a_turn_or_while_paused(harness: Harness) -> None:
    harness.clock.now += 10 * HOUR
    await harness.bridge.maybe_rollover()
    assert harness.session.prompts == []
    await handoff_turn(harness)
    harness.bridge.paused = True
    harness.clock.now += 10 * HOUR
    await harness.bridge.maybe_rollover()
    assert len(harness.session.prompts) == 1


async def test_no_rollover_when_disabled(tmp_path: Path) -> None:
    h = Harness(tmp_path, idle_reset=0)
    await handoff_turn(h)
    h.clock.now += 100 * HOUR
    await h.bridge.maybe_rollover()
    assert len(h.session.prompts) == 1


async def test_a_failed_handoff_keeps_the_session_and_retries_later(harness: Harness) -> None:
    await handoff_turn(harness)

    async def broken(session: FakeSession, prompt: str) -> TurnResult:
        return TurnResult("session-1", "API overloaded")

    harness.session.script = broken
    harness.clock.now += 7 * HOUR
    await harness.bridge.maybe_rollover()
    assert harness.bridge.session_id == "session-1" and None not in harness.session.connects
    await harness.bridge.maybe_rollover()
    assert len(harness.session.prompts) == 2
    harness.clock.now += 1.01 * HOUR
    await harness.bridge.maybe_rollover()
    assert len(harness.session.prompts) == 3


async def test_stop_interrupts_the_handoff(harness: Harness) -> None:
    await handoff_turn(harness)

    async def slow(session: FakeSession, prompt: str) -> TurnResult:
        await session.interrupted.wait()
        return TurnResult("session-1", interrupted=True)

    harness.session.script = slow
    harness.clock.now += 7 * HOUR
    task = asyncio.create_task(harness.bridge.maybe_rollover())
    await settle()
    await harness.say(OWNER, "!stop tsugumi-minecraft")
    await task
    assert harness.bridge.session_id == "session-1"


async def test_restart_counts_idle_time_from_the_saved_turn(tmp_path: Path) -> None:
    first = Harness(tmp_path)
    await handoff_turn(first)
    second = Harness(tmp_path)
    second.clock.now = first.clock.now + 7 * HOUR
    await second.bridge.start()
    assert second.bridge.rollover_due()
    # A state file from before last_turn existed: idle time counts from the restart.
    (tmp_path / "state/session.json").write_text(json.dumps({"session_id": "session-1"}))
    third = Harness(tmp_path)
    third.clock.now = first.clock.now + 7 * HOUR
    await third.bridge.start()
    assert not third.bridge.rollover_due()
    assert "new session" not in (await _one(third))


async def _one(h: Harness) -> str:
    await h.say(ALICE, "@me hi", mention=True)
    await run_turn(h)
    return h.session.prompts[-1]


async def test_a_fresh_start_includes_the_handoff(harness: Harness) -> None:
    write_handoff(harness, "x" * 10_000)
    prompt = await _one(harness)
    assert "truncated at 8192 bytes" in prompt


async def test_context_backlog_is_capped(harness: Harness) -> None:
    for i in range(60):
        await harness.say(CAROL, f"chatter {i}")
    await harness.say(ALICE, "@me catch up", mention=True)
    assert "61 unread" in (await _status(harness))
    await run_turn(harness)
    prompt = harness.session.prompts[-1]
    assert "(10 earlier context messages omitted" in prompt
    assert "chatter 9:" not in prompt and "chatter 10" in prompt and "catch up" in prompt


async def _status(h: Harness) -> str:
    await h.say(ALICE, "!status")
    return h.chat.texts()[-1]


async def test_a_waiting_trigger_is_served_before_a_rollover(harness: Harness) -> None:
    await handoff_turn(harness)
    harness.clock.now += 7 * HOUR
    await harness.say(ALICE, "@me back again", mention=True)
    assert not harness.bridge.rollover_due()
    await run_turn(harness)
    assert harness.bridge.session_id == "session-1"


async def test_a_session_lost_mid_turn_is_not_rolled_over_before_it_is_used(harness: Harness) -> None:
    await handoff_turn(harness)

    async def crash(session: FakeSession, prompt: str) -> TurnResult:
        session.fail_resume = True
        raise ConnectionError("CLI exited")

    harness.session.script = crash
    await harness.say(ALICE, "@me go", mention=True)
    await run_turn(harness)
    assert harness.bridge.session_id is None and harness.bridge.fresh
    harness.clock.now += 7 * HOUR
    assert not harness.bridge.rollover_due()


# --- ask_agent -------------------------------------------------------------------


def from_lab(h: Harness, content: str, reply_to: str | None, message_id: str = "r1") -> Incoming:
    return Incoming(message_id, CHANNEL, None, h.author(LAB), content, reply_to_id=reply_to)


async def test_ask_agent_waits_for_the_reply_to_its_question(tmp_path: Path) -> None:
    h = Harness(tmp_path, ask_agents=["tsugumi-lab"])
    task = asyncio.create_task(h.bridge.tool_ask_agent(
        {"agent": "tsugumi-lab", "headline": "Which view-distance?", "overview": "for erisia"}))
    await settle()
    question_id, question = list(h.chat.sent.items())[-1]
    assert question.content.startswith(f"<@{LAB}> ❓ **Which view-distance?**")
    assert question.mention_users == (LAB,)
    # Its status message, a human's reply, and a lab post elsewhere are not the answer.
    await h.bridge.on_message(from_lab(h, "⚙ tsugumi-lab · working for tsugumi-minecraft · 00:01", question_id, "s1"))
    await h.bridge.on_message(Incoming("x1", CHANNEL, None, h.author(ALICE), "10?", reply_to_id=question_id))
    await h.bridge.on_message(from_lab(h, "💬 **unrelated**", None, "x2"))
    await settle()
    assert not task.done()
    await h.bridge.on_message(from_lab(h, "📄 **12 chunks**", question_id))
    result = await task
    assert result.startswith("tsugumi-lab answered (message r1):\n📄 **12 chunks**")
    assert "unread: 2" in result  # the others arrive as context; the status message is dropped
    assert h.bridge.waiters == {}


async def test_ask_agent_times_out(tmp_path: Path) -> None:
    h = Harness(tmp_path, ask_agents=["tsugumi-lab"])
    result = await h.bridge.tool_ask_agent({"agent": "tsugumi-lab", "headline": "hello?", "timeout_minutes": 0.0001})
    assert result.startswith("No answer from tsugumi-lab")
    question_id = list(h.chat.sent)[-1]
    await h.bridge.on_message(from_lab(h, "📄 **late**", question_id))
    assert "late" in h.bridge.buffer[-1].line


async def test_ask_agent_is_stopped_and_limited(tmp_path: Path) -> None:
    h = Harness(tmp_path, ask_agents=["tsugumi-lab"])
    with pytest.raises(ToolError, match="you can ask: tsugumi-lab"):
        await h.bridge.tool_ask_agent({"agent": "saya", "headline": "hi"})
    task = asyncio.create_task(h.bridge.tool_ask_agent({"agent": "tsugumi-lab", "headline": "hi"}))
    await settle()
    await h.bridge.stop("stopped by baughn")
    with pytest.raises(ToolError, match="stopped by baughn"):
        await task


@pytest.mark.parametrize("names", [["nobody"], ["tsugumi-minecraft"]])
def test_ask_agents_must_be_other_roster_agents(tmp_path: Path, names: list[str]) -> None:
    with pytest.raises(ConfigError):
        parse(config_data(ask_agents=names), tmp_path)


class Channel:
    """One channel for several bridges, keeping mentions and replies."""

    def __init__(self) -> None:
        self.bridges: dict[str, Bridge] = {}
        self.authors: dict[str, str] = {}
        self.counter = itertools.count(1)

    def chat(self, sender_id: str) -> FakeChat:
        channel = self

        class ChannelChat(FakeChat):
            async def send(self, message: Outgoing) -> str:
                message_id = f"c{next(channel.counter)}"
                self.sent[message_id] = message
                channel.authors[message_id] = sender_id
                for bridge in channel.bridges.values():
                    author = classify(bridge.config, sender_id, "x", is_bot=True, webhook_id=None, is_admin=False)
                    await bridge.on_message(Incoming(
                        message_id, CHANNEL, None, author, message.content,
                        mentions=frozenset(message.mention_users), reply_to_id=message.reply_to,
                        reply_to_author=channel.authors.get(message.reply_to or "")))
                return message_id

        return ChannelChat()


async def test_an_owner_only_agent_asks_another_and_gets_the_answer(tmp_path: Path) -> None:
    channel = Channel()
    answers: list[str] = []

    async def asker(session: FakeSession, prompt: str) -> TurnResult:
        answers.append(await session.bridge.tool_ask_agent(
            {"agent": "tsugumi-minecraft", "headline": "What view-distance does erisia run?"}))
        return TurnResult("s-asker")

    async def answerer(session: FakeSession, prompt: str) -> TurnResult:
        question = re.search(r"\[(c\d+)\] tsugumi-lab \(agent, agent\), may ask you to act", prompt)
        assert question is not None, prompt
        await session.bridge.tool_post({"kind": "report", "headline": "12 chunks", "reply_to": question[1]})
        return TurnResult("s-answerer")

    # saya's production shape: agents may trigger it, so the answerer's status
    # message (a reply to the question) must not start a turn.
    identities = (("tsugumi-lab", LAB, asker, dict(triggers=["owner", "agent"], approvers=["owner"],
                                                   ask_agents=["tsugumi-minecraft"])),
                  ("tsugumi-minecraft", ME, answerer, {}))
    for identity, discord_id, script, extra in identities:
        (tmp_path / identity / "work").mkdir(parents=True)
        (tmp_path / identity / "state").mkdir()
        config = parse(config_data(id=identity, workdir=str(tmp_path / identity / "work"), **extra),
                       tmp_path / identity / "state")
        bridge = Bridge(config, channel.chat(discord_id), FakeSession)
        assert isinstance(bridge.session, FakeSession)
        bridge.session.script = script
        channel.bridges[identity] = bridge
    runners = [asyncio.create_task(b.run()) for b in channel.bridges.values()]
    lab = channel.bridges["tsugumi-lab"]
    owner = classify(lab.config, OWNER, "baughn", is_bot=False, webhook_id=None, is_admin=True)
    await lab.on_message(Incoming("h1", CHANNEL, None, owner, "ask it", mentions=frozenset({LAB})))
    for _ in range(50):
        await settle()
    for runner in runners:
        runner.cancel()
    assert len(answers) == 1 and "tsugumi-minecraft answered" in answers[0] and "12 chunks" in answers[0]
    assert isinstance(lab.session, FakeSession) and len(lab.session.prompts) == 1
    assert not any(e.trigger for e in lab.buffer)


async def test_ask_agent_refuses_the_agent_that_is_waiting_on_us(tmp_path: Path) -> None:
    h = Harness(tmp_path, ask_agents=["tsugumi-lab"])

    async def script(session: FakeSession, prompt: str) -> TurnResult:
        with pytest.raises(ToolError, match="waiting for your reply"):
            await session.bridge.tool_ask_agent({"agent": "tsugumi-lab", "headline": "and you?"})
        return TurnResult("s")

    h.session.script = script
    await h.say(LAB, "a question for you", mention=True)
    await run_turn(h)
    assert len(h.session.prompts) == 1


def test_agents_but_not_admins_trigger_a_saya_like_identity(tmp_path: Path) -> None:
    config = parse(config_data(triggers=["owner", "agent"], approvers=["owner"]), tmp_path)
    admin = message(config, ALICE, "@me fix it", mention=True)
    agent = message(config, LAB, "@me fix it", mention=True)
    assert route(config, admin, bot_streak=0, paused=False).route is Route.CONTEXT
    assert route(config, agent, bot_streak=0, paused=False).route is Route.TRIGGER
    assert not may_approve(config, admin.author)


class Commands:
    def __init__(self, states: list[str], commit: str = "a" * 40) -> None:
        self.calls: list[tuple[str, ...]] = []
        self.states = states
        self.commit = commit

    async def __call__(self, *argv: str) -> tuple[int, str]:
        self.calls.append(argv)
        if argv[:1] == ("git",):
            return 0, self.commit + "\n"
        if argv[:3] == ("systemctl", "show", "-P") and argv[3] == "ActiveState":
            return 0, (self.states.pop(0) if self.states else "inactive") + "\n"
        if argv[:3] == ("systemctl", "show", "-P"):
            return 0, "success\n"
        return 0, ""


def ship_harness(tmp_path: Path) -> Harness:
    logs = tmp_path / "logs"
    logs.mkdir()
    return Harness(tmp_path, ship={"unit": "agent-ship", "repo": str(tmp_path / "repo"), "logs": str(logs)})


async def test_ship_starts_the_unit_and_reports_its_result(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("agent_bridge.bridge.SHIP_POLL", 0)
    h = ship_harness(tmp_path)
    commands = Commands(["activating", "active", "active"])
    h.bridge.run_command = commands
    (tmp_path / "logs" / f"{'a' * 40}.log").write_text("pushed; deployed saya")
    result = await h.bridge.tool_ship({"bookmark": "tsugumi-minecraft/fix-it"})
    unit = f"agent-ship@{'a' * 40}.service"
    assert ("agent-publish",) in commands.calls
    assert ("systemctl", "start", "--no-block", unit) in commands.calls
    assert result.startswith(f"{unit}: success\npushed; deployed saya")


async def test_ship_refuses_other_bookmarks_and_bad_hashes(tmp_path: Path) -> None:
    h = ship_harness(tmp_path)
    h.bridge.run_command = Commands([], commit="not-a-hash")
    for bookmark in ("master", "tsugumi-lab/x", "tsugumi-minecraft/../x"):
        with pytest.raises(ToolError, match="bookmark must be"):
            await h.bridge.tool_ship({"bookmark": bookmark})
    with pytest.raises(ToolError, match="no bookmark"):
        await h.bridge.tool_ship({"bookmark": "tsugumi-minecraft/x"})
