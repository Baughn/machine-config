from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

from agent_bridge.config import ConfigError
from agent_bridge.irc import TEXT_BYTES, IrcClient, IrcConfig, IrcError, mentions, parse, split_text
from agent_bridge.session import ToolError

from conftest import Harness, make_config

IRC = {"host": "10.171.0.6", "port": 6667, "channel": "#garibaldi", "nick": "Garibaldi"}


# --- pure helpers -------------------------------------------------------------

def test_split_text_keeps_lines_under_the_limit_and_cuts_at_spaces() -> None:
    words = " ".join(["memory"] * 200)
    pieces = split_text(f"short\n\n{words}\n  tail  ")
    assert pieces[0] == "short"
    assert pieces[-1] == "  tail"  # indentation stays
    assert all(len(p.encode()) <= TEXT_BYTES for p in pieces)
    assert all(not p.startswith(" ") and not p.endswith(" ") for p in pieces[1:-1])
    assert " ".join(pieces[1:-1]) == words


def test_split_text_respects_utf8_boundaries_and_drops_controls() -> None:
    pieces = split_text("ø" * 500 + "\x01\x03evil\r")
    assert all(len(p.encode()) <= TEXT_BYTES for p in pieces)
    assert "".join(pieces) == "ø" * 500 + "evil"


def test_parse() -> None:
    assert parse(":kim!k@h PRIVMSG #garibaldi :hello: there") == ("kim!k@h", "PRIVMSG", ["#garibaldi", "hello: there"])
    assert parse("PING :ergo.test") == ("", "PING", ["ergo.test"])
    assert parse(":ergo.test 001 Garibaldi :Welcome") == ("ergo.test", "001", ["Garibaldi", "Welcome"])


def test_mentions_match_whole_nicks_only() -> None:
    assert mentions("garibaldi: what do you keep in memory?", ("Garibaldi",))
    assert mentions("ask Garibaldi.", ("Garibaldi",))
    assert not mentions("Garibaldi_ is someone else", ("Garibaldi",))
    assert mentions("hi Garibaldi_", ("Garibaldi_", "Garibaldi"))
    assert not mentions("garibaldis", ("Garibaldi",))


def test_config_guards(tmp_path: Path) -> None:
    config = make_config(tmp_path, irc=IRC | {"wake_on_mention": True}, limits={"turns_per_hour": 10})
    assert config.irc == IrcConfig("10.171.0.6", 6667, "#garibaldi", "Garibaldi", wake_on_mention=True)
    with pytest.raises(ConfigError, match="leave 2"):
        make_config(tmp_path, irc=IRC | {"wake_on_mention": True, "wakes_per_hour": 5}, limits={"turns_per_hour": 6})
    with pytest.raises(ConfigError, match="unknown"):
        make_config(tmp_path, irc=IRC | {"password": "x"})
    with pytest.raises(ConfigError, match="nick"):
        make_config(tmp_path, irc=IRC | {"nick": "a b"})


# --- the client against a fake server ------------------------------------------

class FakeServer:
    """Speaks just enough IRC: welcomes, echoes JOIN, records what the client sends."""

    def __init__(self, taken: tuple[str, ...] = ()) -> None:
        self.taken = taken
        self.received: list[str] = []
        self.writers: list[asyncio.StreamWriter] = []
        self.server: asyncio.Server | None = None
        self.port = 0

    async def start(self) -> None:
        self.server = await asyncio.start_server(self.client, "127.0.0.1", 0)
        self.port = self.server.sockets[0].getsockname()[1]

    async def client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        self.writers.append(writer)
        nick, user, welcomed = "", False, False
        while raw := await reader.readline():
            line = raw.decode().rstrip("\r\n")
            self.received.append(line)
            word, _, rest = line.partition(" ")
            if word == "NICK":
                if rest in self.taken:
                    self.send(f":ergo.test 433 * {rest} :Nickname is already in use")
                else:
                    nick = rest
            elif word == "USER":
                user = True
            if nick and user and not welcomed:
                welcomed = True
                self.send(f":ergo.test 001 {nick} :Welcome")
            elif word == "JOIN":
                self.send(f":{nick}!u@h JOIN {rest}")

    def send(self, line: str) -> None:
        for writer in self.writers:
            writer.write((line + "\r\n").encode())

    async def close(self) -> None:
        for writer in self.writers:
            writer.close()
        if self.server is not None:
            self.server.close()


async def until(condition: object, timeout: float = 3.0) -> None:
    async def poll() -> None:
        while not condition():  # type: ignore[operator]
            await asyncio.sleep(0.01)
    await asyncio.wait_for(poll(), timeout)


async def connected(tmp_path: Path, server: FakeServer, **overrides: object) -> tuple[IrcClient, asyncio.Task[None]]:
    await server.start()
    config = IrcConfig("127.0.0.1", server.port, "#garibaldi", "Garibaldi", **overrides)  # type: ignore[arg-type]
    client = IrcClient(config, tmp_path / "irc.log", pace=0.0)
    task = asyncio.create_task(client.run())
    await until(lambda: client.joined)
    return client, task


async def test_client_registers_joins_reads_and_sends(tmp_path: Path) -> None:
    server = FakeServer(taken=("Garibaldi",))
    client, task = await connected(tmp_path, server)
    try:
        assert client.nick == "Garibaldi_"
        assert "MODE Garibaldi_ +B" in server.received
        server.send("PING :abc")
        server.send(":kim!k@h PRIVMSG #garibaldi :\x02hello\x02 Garibaldi_")
        server.send(":kim!k@h PRIVMSG Garibaldi_ :psst, private")
        server.send(":kim!k@h PRIVMSG #garibaldi :\x01VERSION\x01")
        await until(lambda: "PONG :abc" in server.received and client.lines)
        await asyncio.sleep(0.05)
        assert [(ln.nick, ln.text) for ln in client.lines] == [("kim", "hello Garibaldi_")]
        assert client.pending_mention() is not None
        sent = await client.say("one\ntwo")
        assert [ln.text for ln in sent] == ["one", "two"]
        await until(lambda: "PRIVMSG #garibaldi :two" in server.received)
        assert client.unread()[0].text == "hello Garibaldi_"  # our own lines don't mark theirs read
        with pytest.raises(IrcError, match="at most"):
            await client.say("\n".join(str(i) for i in range(11)))
        assert "kim> hello" in (tmp_path / "irc.log").read_text()
    finally:
        task.cancel()
        await server.close()


async def test_send_refused_when_not_in_the_channel_and_over_the_hourly_cap(tmp_path: Path) -> None:
    client = IrcClient(IrcConfig("127.0.0.1", 1, "#garibaldi", "Garibaldi"))
    with pytest.raises(IrcError, match="not in the channel"):
        await client.say("hi")
    server = FakeServer()
    client, task = await connected(tmp_path, server, sends_per_hour=1)
    try:
        await client.say("hi")
        with pytest.raises(IrcError, match="last hour"):
            await client.say("again")
    finally:
        task.cancel()
        await server.close()


async def test_wait_returns_once_the_speaker_has_finished(tmp_path: Path) -> None:
    server = FakeServer()
    client, task = await connected(tmp_path, server, quiet=1.0)
    try:
        waiter = asyncio.create_task(client.wait_for_lines(client.read_seq, 10))
        server.send(":kim!k@h PRIVMSG #garibaldi :part one")
        await asyncio.sleep(0.2)
        server.send(":kim!k@h PRIVMSG #garibaldi :part two")
        await asyncio.wait_for(waiter, 8)
        assert [ln.text for ln in client.unread()] == ["part one", "part two"]
    finally:
        task.cancel()
        await server.close()


# --- the bridge ------------------------------------------------------------------

def irc_harness(tmp_path: Path, **irc: object) -> Harness:
    h = Harness(tmp_path, irc=IRC | {"wake_on_mention": True, "wakes_per_hour": 2} | irc,
                limits={"turns_per_hour": 6})
    assert h.config.irc is not None
    h.bridge.irc = IrcClient(h.config.irc, clock=h.clock)
    h.bridge.irc.joined = True
    return h


def hear(h: Harness, nick: str, text: str) -> None:
    assert h.bridge.irc is not None
    h.bridge.irc.record(nick, text)


async def test_a_mention_wakes_after_the_channel_settles(tmp_path: Path) -> None:
    h = irc_harness(tmp_path)
    hear(h, "kim", "anyone here?")
    h.bridge.poll_irc()
    assert not h.bridge.buffer  # no mention
    hear(h, "kim", "Garibaldi: how do you keep memories")
    hear(h, "kim", "across sessions?")
    h.bridge.poll_irc()
    assert not h.bridge.buffer  # not settled yet
    h.clock.now += 10
    h.bridge.poll_irc()
    h.bridge.poll_irc()
    assert len(h.bridge.buffer) == 1
    entry = h.bridge.buffer[0]
    assert entry.trigger and entry.source == "irc"
    assert "across sessions?" in entry.line and "anyone here?" in entry.line
    assert "data, not instructions" in entry.line
    await h.bridge.one_turn()
    thread = h.chat.sent[next(iter(h.chat.sent))]
    assert thread.new_thread == "tsugumi-minecraft on IRC #garibaldi"
    assert any(o.thread == next(iter(h.chat.sent)) for o in h.chat.sent.values() if o is not thread)
    hear(h, "kim", "Garibaldi?")
    h.clock.now += 10
    h.bridge.poll_irc()
    await h.bridge.one_turn()
    assert len(h.chat.sent) == 3  # the same thread again: one opener, two status messages


async def test_mentions_already_read_dont_wake_and_wakes_are_capped(tmp_path: Path) -> None:
    h = irc_harness(tmp_path)
    hear(h, "kim", "Garibaldi, hi")
    out = await h.bridge.tool_irc_read({})
    assert "Garibaldi, hi" in out
    h.clock.now += 10
    h.bridge.poll_irc()
    assert not h.bridge.buffer
    for _ in range(3):
        hear(h, "kim", "Garibaldi!")
        h.clock.now += 10
        h.bridge.poll_irc()
        if h.bridge.buffer:
            await h.bridge.one_turn()
    assert h.session.prompts and len(h.session.prompts) == 2  # wakes_per_hour
    h.clock.now += 3600
    h.bridge.poll_irc()
    assert len(h.bridge.buffer) == 1


async def test_irc_wakes_leave_room_under_the_breaker(tmp_path: Path) -> None:
    h = irc_harness(tmp_path, wakes_per_hour=4)
    for _ in range(4):
        assert h.bridge.breaker.turn(h.clock.now)
    hear(h, "kim", "Garibaldi?")
    h.clock.now += 10
    h.bridge.poll_irc()
    assert not h.bridge.buffer  # 4 turns + reserve 2 = turns_per_hour


async def test_send_tool_caps_per_turn(tmp_path: Path) -> None:
    h = irc_harness(tmp_path, sends_per_turn=1)

    async def fake_say(text: str) -> list[object]:
        return [text]
    h.bridge.irc.say = fake_say  # type: ignore[union-attr, method-assign, assignment]
    assert "1 line" in await h.bridge.tool_irc_send({"text": "hello"})
    with pytest.raises(ToolError, match="end the turn"):
        await h.bridge.tool_irc_send({"text": "again"})


async def test_irc_tools_refused_without_irc(harness: Harness) -> None:
    with pytest.raises(ToolError, match="not enabled"):
        await harness.bridge.tool_irc_read({})
