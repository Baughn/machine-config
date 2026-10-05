from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
import itertools
from pathlib import Path
from typing import Any

import pytest

from agent_bridge.bridge import Bridge
from agent_bridge.config import Config, parse
from agent_bridge.policy import Attachment, Author, Incoming, Kind, classify
from agent_bridge.render import ChatError, Outgoing
from agent_bridge.session import TurnResult

CHANNEL = "50"
OWNER, ALICE, CAROL, MALLORY = "1", "2", "3", "4"
ME, LAB = "100", "101"
WATCHDOG = "900"


def config_data(**overrides: Any) -> dict[str, Any]:
    data: dict[str, Any] = {
        "id": "tsugumi-minecraft",
        "workdir": "/srv/agent",
        "channel_id": CHANNEL,
        "triggers": ["owner", "admin", "agent"],
        "approvers": ["owner", "admin"],
        "roster": {
            "guild_id": "7",
            "admin_role_id": "8",
            "watchdog_webhook_id": WATCHDOG,
            "humans": {"baughn": {"discord_id": OWNER, "role": "owner"},
                       "alice": {"discord_id": ALICE, "role": "admin"}},
            "agents": {"tsugumi-minecraft": {"discord_id": ME, "description": "ops"},
                       "tsugumi-lab": {"discord_id": LAB, "description": "lab"}},
        },
    }
    data.update(overrides)
    return data


def make_config(tmp: Path | None = None, **overrides: Any) -> Config:
    data = config_data(**overrides)
    if tmp is not None and "workdir" not in overrides:
        data["workdir"] = str(tmp / "work")
    return parse(data, tmp / "state" if tmp else Path("/var/lib/test"))


def author(config: Config, who: str, *, admin: bool = True) -> Author:
    names = {OWNER: "baughn", ALICE: "alice", CAROL: "carol", MALLORY: "mallory", ME: "me", LAB: "lab"}
    if who == "watchdog":
        return classify(config, "55", "minecraft-watch", is_bot=True, webhook_id=WATCHDOG, is_admin=False)
    if who == "stranger-bot":
        return classify(config, "56", "botty", is_bot=True, webhook_id=None, is_admin=False)
    return classify(config, who, names[who], is_bot=who in (ME, LAB), webhook_id=None, is_admin=admin)


counter = itertools.count(1000)


def message(config: Config, who: str, content: str = "hi", *, mention: bool = False,
            admin: bool = True, channel: str = CHANNEL, reply_to_author: str | None = None,
            role_mentions: frozenset[str] = frozenset(),
            attachments: tuple[Attachment, ...] = (), thread: str | None = None,
            thread_name: str | None = None, thread_owner: str | None = None) -> Incoming:
    return Incoming(
        id=str(next(counter)), channel_id=channel, thread_id=thread,
        author=author(config, who, admin=admin), content=content,
        mentions=frozenset({ME}) if mention else frozenset(),
        role_mentions=role_mentions, reply_to_author=reply_to_author, attachments=attachments,
        thread_name=thread_name, thread_owner=thread_owner)


class FakeChat:
    def __init__(self) -> None:
        self.ids = itertools.count(1)
        self.sent: dict[str, Outgoing] = {}
        self.edits: dict[str, list[str]] = {}
        self.approvals: list[str] = []
        self.questions: dict[str, list[dict[str, Any]]] = {}
        self.backlog: list[Incoming] = []
        self.downloads: list[Path] = []
        self.reactions: list[tuple[str, str]] = []
        self.histories: list[str | None] = []  # the thread each history call read
        self.refuse: str | None = None  # a ChatError every send raises
        self.deleted: list[str] = []

    def new_id(self) -> str:
        return f"m{next(self.ids)}"

    async def send(self, message: Outgoing) -> str:
        if self.refuse is not None:
            raise ChatError(self.refuse)
        message_id = self.new_id()
        self.sent[message_id] = message
        return message_id

    async def edit(self, message_id: str, content: str) -> None:
        self.edits.setdefault(message_id, []).append(content)

    async def delete(self, message_id: str) -> None:
        self.deleted.append(message_id)

    async def approve(self, message: Outgoing) -> str:
        message_id = await self.send(message)
        self.approvals.append(message_id)
        return message_id

    async def ask(self, message: Outgoing, questions: list[dict[str, Any]]) -> str:
        message_id = await self.send(message)
        self.questions[message_id] = questions
        return message_id

    async def react(self, message_id: str, emoji: str) -> None:
        self.reactions.append((message_id, emoji))

    async def history(self, limit: int, thread: str | None = None) -> list[Incoming]:
        self.histories.append(thread)
        return self.backlog[-limit:]

    def url(self, message_id: str) -> str:
        return f"https://discord/{message_id}"

    async def download(self, attachment: Attachment, path: Path) -> None:
        path.write_text(f"downloaded {attachment.url}")
        self.downloads.append(path)

    def texts(self) -> list[str]:
        return [m.content for m in self.sent.values()]

    def final(self, message_id: str) -> str:
        return self.edits.get(message_id, [self.sent[message_id].content])[-1]


Script = Callable[["FakeSession", str], Awaitable[TurnResult]]


async def silent(session: FakeSession, prompt: str) -> TurnResult:
    return TurnResult("session-1")


class FakeSession:
    def __init__(self, bridge: Bridge) -> None:
        self.bridge = bridge
        self.script: Script = silent
        self.prompts: list[str] = []
        self.connects: list[str | None] = []
        self.interrupted = asyncio.Event()
        self.disconnects = 0
        self.fail_resume = False
        self.efforts: list[str] = []

    async def connect(self, resume: str | None) -> None:
        self.connects.append(resume)
        if resume is not None and self.fail_resume:
            raise RuntimeError("no such session")

    async def turn(self, prompt: str) -> TurnResult:
        self.prompts.append(prompt)
        self.interrupted.clear()
        return await self.script(self, prompt)

    async def interrupt(self) -> None:
        self.interrupted.set()

    async def disconnect(self) -> None:
        self.disconnects += 1

    async def set_effort(self, level: str) -> None:
        self.efforts.append(level)


class Clock:
    def __init__(self) -> None:
        self.now = 1_000_000.0

    def __call__(self) -> float:
        return self.now


class Harness:
    def __init__(self, tmp: Path, **overrides: Any) -> None:
        (tmp / "work").mkdir(exist_ok=True)
        (tmp / "state").mkdir(exist_ok=True)
        self.config = make_config(tmp, **overrides)
        self.chat = FakeChat()
        self.clock = Clock()
        self.bridge = Bridge(self.config, self.chat, FakeSession, clock=self.clock, tokens=("tok-discord-secret",))
        assert isinstance(self.bridge.session, FakeSession)
        self.session: FakeSession = self.bridge.session

    def msg(self, who: str, content: str = "hi", **kwargs: Any) -> Incoming:
        return message(self.config, who, content, **kwargs)

    async def say(self, who: str, content: str = "hi", **kwargs: Any) -> Incoming:
        incoming = self.msg(who, content, **kwargs)
        await self.bridge.on_message(incoming)
        return incoming

    def author(self, who: str, **kwargs: Any) -> Author:
        return author(self.config, who, **kwargs)


@pytest.fixture
def harness(tmp_path: Path) -> Harness:
    return Harness(tmp_path)


__all__ = ["Kind"]
