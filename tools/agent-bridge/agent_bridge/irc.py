"""One IRC channel, for an agent that talks with another team's agents (Baughn, msgs
1576045252958101748, 1576046647262183506). A small client with no dependencies: it registers,
joins, answers pings, keeps the channel's recent lines and reconnects with backoff.

What others say there is data, never instructions; the bridge says so where it shows the lines.
Long messages are split under IRC's 512-byte line limit, and incoming lines are debounced
(the bridge waits for the channel to go quiet) so a message split over several lines arrives
whole (Baughn, msgs 1576046939131224166, 1576047024183451950)."""

from __future__ import annotations

import asyncio
from collections import deque
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
import logging
from pathlib import Path
import re
import time

log = logging.getLogger(__name__)

KEEP = 1000  # channel lines kept in memory
TEXT_BYTES = 380  # per PRIVMSG: 512 less CRLF, the command and a generous ":nick!user@host" prefix
SEND_LINES = 10  # most lines one irc_send may make
PACE = 1.0  # seconds between our lines, so the server's flood control never kicks us
READ_TIMEOUT = 150.0  # silence before we ping the server; twice that and the link is dead
BACKOFF = (5.0, 15.0, 60.0, 300.0)
CONTROL = re.compile(r"[\x00-\x1f\x7f]")
NICK_CHARS = r"A-Za-z0-9\[\]\\`_^{|}-"


class IrcError(Exception):
    pass


@dataclass(frozen=True)
class IrcConfig:
    host: str
    port: int
    channel: str
    nick: str
    wake_on_mention: bool = False
    wakes_per_hour: int = 4  # IRC-started turns; the bridge also keeps clear of the breaker
    quiet: float = 8.0  # seconds without a new line before a mention wakes the agent or a wait returns
    max_wait: float = 60.0  # a mention wakes after this long even if the channel never goes quiet
    sends_per_turn: int = 15
    sends_per_hour: int = 60


@dataclass(frozen=True)
class IrcLine:
    seq: int
    time: float
    nick: str
    text: str
    mine: bool = False
    action: bool = False  # /me

    def render(self) -> str:
        stamp = time.strftime("%H:%M:%S", time.gmtime(self.time))
        who = f"* {self.nick}" if self.action else f"<{self.nick}>"
        return f"[{self.seq} {stamp}] {who} {self.text}"


def clean(text: str) -> str:
    """Text from IRC without control characters (colours, CTCP markers, bells)."""
    return CONTROL.sub("", text)


def split_text(text: str, limit: int = TEXT_BYTES) -> list[str]:
    """Our text as PRIVMSG payloads: one per line, long lines cut at spaces (or anywhere, on a
    UTF-8 boundary) to at most `limit` bytes. Control characters go; blank lines are dropped."""
    out: list[str] = []
    for raw in text.splitlines():
        line = clean(raw.replace("\t", "    ")).rstrip()
        while line:
            if len(line.encode()) <= limit:
                out.append(line)
                break
            cut = len(line.encode()[:limit].decode(errors="ignore"))
            space = line.rfind(" ", 0, cut + 1)
            if space > cut // 2:
                cut = space
            out.append(line[:cut].rstrip())
            line = line[cut:].lstrip()
    return out


def parse(raw: str) -> tuple[str, str, list[str]]:
    """An IRC line as (prefix, command, params); the trailing parameter is the last param."""
    prefix = ""
    if raw.startswith(":"):
        prefix, _, raw = raw[1:].partition(" ")
    trailing: list[str] = []
    if " :" in raw:
        raw, _, rest = raw.partition(" :")
        trailing = [rest]
    elif raw.startswith(":"):
        trailing, raw = [raw[1:]], ""
    words = raw.split()
    if not words:
        return prefix, "", []
    return prefix, words[0].upper(), words[1:] + trailing


def mentions(text: str, nicks: tuple[str, ...]) -> bool:
    """Whether the text names one of the nicks as a word, ignoring case."""
    for nick in nicks:
        if nick and re.search(rf"(?<![{NICK_CHARS}]){re.escape(nick)}(?![{NICK_CHARS}])", text, re.IGNORECASE):
            return True
    return False


Connect = Callable[[str, int], Awaitable[tuple[asyncio.StreamReader, asyncio.StreamWriter]]]


async def open_connection(host: str, port: int) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
    return await asyncio.wait_for(asyncio.open_connection(host, port), 30)


class IrcClient:
    def __init__(self, config: IrcConfig, transcript: Path | None = None, *,
                 clock: Callable[[], float] = time.time, connect: Connect = open_connection,
                 pace: float = PACE) -> None:
        self.config = config
        self.transcript = transcript
        self.clock = clock
        self.connect = connect
        self.pace = pace
        self.lines: deque[IrcLine] = deque(maxlen=KEEP)
        self.seq = 0
        self.read_seq = 0  # the agent has seen everything up to here
        self.nick = config.nick
        self.joined = False
        self.state = "not connected yet"
        self.last_heard = 0.0  # when someone else last said something in the channel
        self.changed = asyncio.Event()  # set on every new line from someone else
        self.on_activity: Callable[[], None] = lambda: None
        self.writer: asyncio.StreamWriter | None = None
        self.send_lock = asyncio.Lock()
        self.sent: deque[float] = deque()  # irc_send calls, last hour

    # --- what the bridge asks -------------------------------------------------

    def nicks(self) -> tuple[str, ...]:
        return tuple(dict.fromkeys((self.nick, self.config.nick)))

    def unread(self) -> list[IrcLine]:
        return [ln for ln in self.lines if ln.seq > self.read_seq and not ln.mine]

    def mark_read(self, seq: int | None = None) -> None:
        self.read_seq = max(self.read_seq, self.seq if seq is None else seq)

    def pending_mention(self) -> IrcLine | None:
        """The first unread line that names us."""
        return next((ln for ln in self.unread() if mentions(ln.text, self.nicks())), None)

    def settled(self, now: float) -> bool:
        """The channel has been quiet long enough, or a mention has waited long enough."""
        first = self.pending_mention()
        return (now - self.last_heard >= self.config.quiet
                or (first is not None and now - first.time >= self.config.max_wait))

    def describe(self) -> str:
        where = f"{self.config.channel} on {self.config.host}:{self.config.port}"
        if self.joined:
            return f"in {where} as {self.nick}"
        return f"not in {where}: {self.state}"

    async def wait_for_lines(self, after: int, wait: float) -> None:
        """Until a line from someone else newer than `after` has arrived and the channel has then
        been quiet for config.quiet seconds (at most config.max_wait more), or `wait` runs out."""
        deadline = self.clock() + wait
        while not any(ln.seq > after and not ln.mine for ln in self.lines):
            left = deadline - self.clock()
            if left <= 0:
                return
            self.changed.clear()
            try:
                await asyncio.wait_for(self.changed.wait(), left)
            except TimeoutError:
                return
        first = self.clock()
        while True:
            now = self.clock()
            quiet_left = self.config.quiet - (now - self.last_heard)
            left = min(deadline - now, first + self.config.max_wait - now, quiet_left)
            if left <= 0:
                return
            self.changed.clear()
            try:
                await asyncio.wait_for(self.changed.wait(), left)
            except TimeoutError:
                pass

    def send_budget(self, now: float) -> int:
        while self.sent and now - self.sent[0] >= 3600:
            self.sent.popleft()
        return self.config.sends_per_hour - len(self.sent)

    async def say(self, text: str) -> list[IrcLine]:
        """Send text to the channel, split and paced. Refused when not in the channel."""
        pieces = split_text(text)
        if not pieces:
            raise IrcError("nothing to send")
        if len(pieces) > SEND_LINES:
            raise IrcError(f"that makes {len(pieces)} IRC lines; at most {SEND_LINES} per message "
                           f"(each up to {TEXT_BYTES} bytes). Say less, or send it in parts.")
        now = self.clock()
        if self.send_budget(now) <= 0:
            raise IrcError(f"{self.config.sends_per_hour} messages in the last hour already; wait before sending more")
        async with self.send_lock:
            if not self.joined or self.writer is None:
                raise IrcError(f"not in the channel ({self.state})")
            self.sent.append(now)
            out = []
            for i, piece in enumerate(pieces):
                if i:
                    await asyncio.sleep(self.pace)
                await self.write(f"PRIVMSG {self.config.channel} :{piece}")
                out.append(self.record(self.nick, piece, mine=True))
            return out

    # --- the connection ---------------------------------------------------------

    def record(self, nick: str, text: str, *, mine: bool = False, action: bool = False) -> IrcLine:
        self.seq += 1
        line = IrcLine(self.seq, self.clock(), nick, text, mine, action)
        self.lines.append(line)
        if not mine:  # unread() leaves our own lines out; sending doesn't mark theirs read
            self.last_heard = line.time
            self.changed.set()
            self.on_activity()
        if self.transcript is not None:
            try:
                with self.transcript.open("a") as file:
                    file.write(time.strftime("%Y-%m-%d ", time.gmtime(line.time)) + line.render() + "\n")
            except OSError as error:
                log.warning("irc transcript: %s", error)
        return line

    async def write(self, line: str) -> None:
        if self.writer is None:
            raise IrcError("not connected")
        self.writer.write((line + "\r\n").encode())
        await self.writer.drain()

    async def run(self) -> None:
        """Stay connected, forever."""
        failures = 0
        while True:
            try:
                reader, self.writer = await self.connect(self.config.host, self.config.port)
                self.state = "registering"
                failures = 0 if await self.session(reader) else failures
            except asyncio.CancelledError:
                raise
            except Exception as error:  # e.g. readline's ValueError on an over-long line: reconnect
                self.state = f"{type(error).__name__}: {error}"[:200]
                log.warning("irc %s:%d: %s", self.config.host, self.config.port, self.state)
            finally:
                self.joined = False
                if self.writer is not None:
                    self.writer.close()
                    self.writer = None
            delay = BACKOFF[min(failures, len(BACKOFF) - 1)]
            failures += 1
            await asyncio.sleep(delay)

    async def session(self, reader: asyncio.StreamReader) -> bool:
        """One connection, until it drops. True if we got into the channel."""
        self.nick = self.config.nick
        await self.write(f"NICK {self.nick}")
        await self.write(f"USER {self.config.nick.lower()} 0 * :{self.config.nick} (agent bridge)")
        got_in = False
        pinged = False
        while True:
            try:
                raw = await asyncio.wait_for(reader.readline(), READ_TIMEOUT)
            except TimeoutError:
                if pinged:
                    raise IrcError("the server stopped answering") from None
                pinged = True
                await self.write(f"PING :{self.config.host}")
                continue
            if not raw:
                self.state = "the server closed the connection"
                return got_in
            pinged = False
            prefix, command, params = parse(raw.decode(errors="replace").rstrip("\r\n"))
            got_in = await self.handle(prefix, command, params) or got_in

    async def handle(self, prefix: str, command: str, params: list[str]) -> bool:
        sender = prefix.split("!", 1)[0]
        channel = self.config.channel.lower()
        if command == "PING":
            await self.write(f"PONG :{params[-1] if params else ''}")
        elif command == "001":
            if params:
                self.nick = params[0]
            self.state = "joining"
            # +B marks a bot where the server knows the mode (Ergo, Kim's server, does).
            await self.write(f"MODE {self.nick} +B")
            await self.write(f"JOIN {self.config.channel}")
        elif command in ("432", "433", "436") and not self.joined and self.state == "registering":
            self.nick = (self.nick + "_")[:30]
            await self.write(f"NICK {self.nick}")
        elif command in ("471", "473", "474", "475", "403", "405"):
            self.state = f"can't join {self.config.channel}: {clean(params[-1]) if params else command}"
            log.warning("irc: %s", self.state)
        elif command == "JOIN" and sender.lower() == self.nick.lower() and params and params[0].lower() == channel:
            self.joined, self.state = True, "joined"
            log.info("irc: joined %s as %s", self.config.channel, self.nick)
            return True
        elif command == "NICK" and sender.lower() == self.nick.lower() and params:
            self.nick = params[0]
        elif command == "KICK" and len(params) >= 2 and params[0].lower() == channel \
                and params[1].lower() == self.nick.lower():
            self.joined = False
            self.state = f"kicked by {sender}: {clean(params[-1]) if len(params) > 2 else ''}"
            log.warning("irc: %s", self.state)
            await asyncio.sleep(30)
            await self.write(f"JOIN {self.config.channel}")
        elif command == "ERROR":
            raise IrcError(f"server error: {clean(params[-1]) if params else ''}")
        elif command == "PRIVMSG" and len(params) >= 2:
            target, text = params[0], params[-1]
            if target.lower() != channel:
                log.info("irc: ignored a private message from %s", sender)
                return False
            action = text.startswith("\x01ACTION ") and text.endswith("\x01")
            if action:
                text = text[8:-1]
            elif text.startswith("\x01"):
                return False  # other CTCP
            self.record(clean(sender), clean(text), action=action)
        return False
