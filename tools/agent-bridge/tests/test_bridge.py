"""The real bridge against a fake chat and a scripted fake session."""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

import pytest

from agent_bridge.approval import STOP, Verdict
from agent_bridge.bridge import Bridge
from agent_bridge.config import parse
from agent_bridge.policy import Attachment, Incoming, classify
from agent_bridge.render import Outgoing
from agent_bridge.session import Permission, ToolError, TurnResult

from conftest import ALICE, CAROL, CHANNEL, LAB, ME, OWNER, FakeChat, FakeSession, Harness, config_data


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
    assert json.loads((harness.config.state / "session.json").read_text()) == {"session_id": "session-1"}
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
    assert json.loads((harness.config.state / "session.json").read_text()) == {"session_id": None}


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
