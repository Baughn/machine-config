"""The bridge: routes channel messages into turns and renders the agent's activity."""

from __future__ import annotations

import asyncio
from collections.abc import Callable
import contextlib
from dataclasses import dataclass
import json
import logging
from pathlib import Path
import time
from typing import Any, Protocol

from . import approval, rcon
from .approval import Cancelled, Pending, Verdict
from .config import Config
from .limits import Breaker, Streak
from .policy import (Attachment, Author, Command, Incoming, Kind, Route, command_applies,
                     may_approve, parse_command, route)
from .prompt import turn_prompt
from .render import (File, Outgoing, PostError, Roots, Status, approval_request, context_line,
                     question_text, render_post, summarize_tool)
from .session import AgentSession, Permission, ToolError, TurnResult

log = logging.getLogger(__name__)

STATUS_INTERVAL = 3.0
KEEP_TURN_LOGS = 50


class Chat(Protocol):
    async def send(self, message: Outgoing) -> str: ...
    async def edit(self, message_id: str, content: str) -> None: ...
    async def approve(self, message: Outgoing) -> str: ...
    async def ask(self, message: Outgoing, questions: list[dict[str, Any]]) -> str: ...
    async def history(self, limit: int) -> list[Incoming]: ...
    async def download(self, attachment: Attachment, path: Path) -> None: ...


@dataclass
class Entry:
    message: Incoming
    trigger: bool
    line: str


class Bridge:
    def __init__(self, config: Config, chat: Chat, session: Callable[[Bridge], AgentSession], *,
                 clock: Callable[[], float] = time.time, tokens: tuple[str, ...] = ()) -> None:
        self.config = config
        self.chat = chat
        self.session = session(self)
        self.clock = clock
        self.tokens = tokens
        self.buffer: list[Entry] = []
        self.paused = False
        self.breaker = Breaker(config.limits)
        self.streak = Streak()
        self.pending: dict[str, Pending] = {}
        self.status: Status | None = None
        self.status_id: str | None = None
        self.last_log: list[str] = []
        self.session_id: str | None = None
        self.wake = asyncio.Event()
        self.lock = asyncio.Lock()
        self.stopping = False
        self.turns = 0

    # --- lifecycle ---------------------------------------------------------

    @property
    def session_file(self) -> Path:
        return self.config.state / "session.json"

    async def start(self) -> None:
        with contextlib.suppress(FileNotFoundError, ValueError, KeyError):
            self.session_id = json.loads(self.session_file.read_text())["session_id"]
        try:
            await self.session.connect(self.session_id)
        except Exception:
            log.exception("resuming session %s failed; starting fresh", self.session_id)
            await self.session.connect(None)
            self.save_session(None)
        history = await self.chat.history(50)
        self.streak = Streak.from_history([m.author.kind for m in history])

    async def reconnect(self) -> None:
        """After the CLI died: a new client on the same session, or a fresh one."""
        with contextlib.suppress(Exception):
            await self.session.disconnect()
        try:
            await self.session.connect(self.session_id)
        except Exception:
            log.exception("resuming session %s failed; starting fresh", self.session_id)
            await self.session.connect(None)
            self.save_session(None)

    def save_session(self, session_id: str | None) -> None:
        self.session_id = session_id
        temporary = self.session_file.with_suffix(".tmp")
        temporary.write_text(json.dumps({"session_id": session_id}))
        temporary.replace(self.session_file)

    async def run(self) -> None:
        while True:
            await self.wake.wait()
            self.wake.clear()
            while not self.paused and not self.breaker.tripped and any(e.trigger for e in self.buffer):
                try:
                    await self.one_turn()
                except Exception:
                    # e.g. Discord refusing the status message. The messages stay
                    # queued; the next trigger retries rather than spinning here.
                    log.exception("turn failed outside the session")
                    break

    # --- inbound -----------------------------------------------------------

    async def on_message(self, message: Incoming) -> None:
        if message.channel_id != self.config.channel_id:
            return
        command = parse_command(message.content)
        if command is not None:
            if may_approve(self.config, message.author) and command_applies(self.config, command):
                await self.command(command, message)
            return
        decision = route(self.config, message, bot_streak=self.streak.count,
                         paused=self.paused or self.breaker.tripped is not None)
        self.streak.observe(message.author.kind)
        log.info("message %s from %s: %s (%s)", message.id, message.author.label,
                 decision.route.value, decision.reason)
        if decision.route is Route.IGNORE:
            return
        paths = await self.fetch_attachments(message) if message.author.kind is Kind.HUMAN else []
        trigger = decision.route is Route.TRIGGER
        self.buffer.append(Entry(message, trigger, context_line(message, trigger, paths)))
        if trigger:
            self.wake.set()

    async def fetch_attachments(self, message: Incoming) -> list[str]:
        paths = []
        inbox = self.config.state / "inbox"
        inbox.mkdir(exist_ok=True)
        for attachment in message.attachments:
            name = Path(attachment.name).name or "attachment"
            if attachment.size > self.config.attachment_limit:
                paths.append(f"{name} (not downloaded: over {self.config.attachment_limit} bytes)")
                continue
            path = inbox / f"{message.id}-{name}"
            try:
                await self.chat.download(attachment, path)
                paths.append(str(path))
            except Exception as error:
                paths.append(f"{name} (download failed: {error})")
        return paths

    async def on_reaction(self, message_id: str, message_is_ours: bool, reactor: Author, emoji: str) -> None:
        if not may_approve(self.config, reactor):
            return
        if emoji == approval.STOP and message_is_ours:
            await self.stop(f"stopped by {reactor.name}")

    async def on_decide(self, message_id: str, reactor: Author, verdict: Verdict) -> str:
        """An Allow/Deny button. Returns a note for the clicker."""
        if not may_approve(self.config, reactor):
            return "Only approvers can decide."
        pending = self.pending.get(message_id)
        if pending is None or pending.questions is not None or pending.future.done():
            return "This request is no longer open."
        pending.decided_by = reactor.name
        pending.decide(verdict)
        return "Allowed." if verdict is Verdict.ALLOW else "Denied."

    async def on_select(self, message_id: str, reactor: Author, index: int, labels: list[str]) -> str:
        """An answer to a question's select menu. Returns a note for the clicker."""
        if not may_approve(self.config, reactor):
            return "Only approvers can answer."
        pending = self.pending.get(message_id)
        if pending is None or pending.questions is None:
            return "This question is no longer open."
        pending.decided_by = reactor.name
        if pending.select(index, labels):
            return "Answer sent."
        return "Recorded; answer the remaining questions too."

    # --- commands ----------------------------------------------------------

    async def command(self, command: Command, message: Incoming) -> None:
        who = message.author.name
        match command.verb:
            case "stop":
                await self.stop(f"stopped by {who}")
            case "pause":
                self.paused = True
                await self.say(f"⏸ {self.config.id} paused by {who}.", message.id)
            case "resume":
                self.paused = False
                self.breaker.reset()
                await self.say(f"▶ {self.config.id} resumed by {who}.", message.id)
                self.wake.set()
            case "reset":
                await self.stop(f"reset by {who}")
                async with self.lock:
                    await self.session.disconnect()
                    self.save_session(None)
                    await self.session.connect(None)
                await self.say(f"🔄 {self.config.id}: new session, started by {who}.", message.id)
            case "status":
                if command.argument == "log":
                    body = "\n".join(self.last_log) or "(no turn yet)"
                    await self.chat.send(Outgoing(f"📜 {self.config.id}: last turn's tool log",
                                                  (File("turn.log", body.encode()),), message.id))
                else:
                    await self.say(self.describe(), message.id)

    def describe(self) -> str:
        state = "working" if self.status else "idle"
        if self.paused:
            state += ", paused"
        if self.breaker.tripped:
            state += f", circuit breaker tripped ({self.breaker.tripped})"
        queued = sum(1 for e in self.buffer if e.trigger)
        return (f"ℹ {self.config.id}: {state}; {queued} queued, {len(self.buffer)} unread, "
                f"{len(self.pending)} pending approvals, {self.turns} turns since start, "
                f"agent streak {self.streak.count}")

    async def say(self, text: str, reply_to: str | None = None) -> None:
        await self.chat.send(Outgoing(text, reply_to=reply_to))

    async def stop(self, reason: str) -> None:
        for pending in list(self.pending.values()):
            pending.cancel(reason)
        if self.status is not None:
            self.stopping = True
            await self.session.interrupt()

    async def trip(self) -> None:
        owner = self.config.roster.owner.discord_id
        await self.chat.send(Outgoing(
            f"<@{owner}> 🧯 {self.config.id} tripped its circuit breaker ({self.breaker.tripped}) "
            f"and is paused. `!resume {self.config.id}` clears it.", mention_users=(owner,)))
        await self.stop("circuit breaker tripped")

    # --- turns -------------------------------------------------------------

    async def one_turn(self) -> None:
        async with self.lock:
            now = self.clock()
            if not self.breaker.turn(now):
                await self.trip()
                return
            entries, self.buffer = self.buffer, []
            trigger = [e for e in entries if e.trigger][-1].message
            status = Status(self.config.id, trigger.author.name, now)
            self.status, self.stopping = status, False
            try:
                self.status_id = await self.chat.send(Outgoing(status.render(now), reply_to=trigger.id))
            except Exception:
                self.status, self.buffer = None, entries + self.buffer
                raise
            ticker = asyncio.create_task(self.tick())
            result: TurnResult | None = None
            try:
                result = await self.session.turn(turn_prompt([e.line for e in entries]))
            except Exception as error:
                log.exception("turn failed")
                result = TurnResult(self.session_id, f"{type(error).__name__}: {error}")
                await self.reconnect()
            finally:
                ticker.cancel()
                for pending in list(self.pending.values()):
                    pending.cancel("the turn ended")
            if result.session_id and result.session_id != self.session_id:
                self.save_session(result.session_id)
            if result.interrupted or self.stopping:
                final = "stopped"
            elif result.error:
                final = "error"
                status.log.append(f"error: {result.error}")
            else:
                final = "done" if status.posts else "silent"
            self.turns += 1
            self.last_log = status.log
            self.write_turn_log(status, final, result)
            self.status = None
            with contextlib.suppress(Exception):
                await self.chat.edit(self.status_id, status.render(self.clock(), final))

    async def tick(self) -> None:
        shown = ""
        while self.status is not None and self.status_id is not None:
            text = self.status.render(self.clock())
            if text != shown:
                with contextlib.suppress(Exception):
                    await self.chat.edit(self.status_id, text)
                shown = text
            await asyncio.sleep(STATUS_INTERVAL)

    def write_turn_log(self, status: Status, final: str, result: TurnResult) -> None:
        turns = self.config.state / "turns"
        turns.mkdir(exist_ok=True)
        stamp = time.strftime("%Y%m%dT%H%M%S", time.gmtime(status.started))
        cost = f"{result.cost_usd:.4f}" if result.cost_usd is not None else "?"
        (turns / f"{stamp}.log").write_text(
            f"for {status.requester}: {final}, session {result.session_id}, cost ${cost}\n"
            + "\n".join(status.log) + "\n")
        for old in sorted(turns.glob("*.log"))[:-KEEP_TURN_LOGS]:
            old.unlink()

    def note(self, line: str) -> None:
        if self.status is not None:
            stamp = time.strftime("%H:%M:%S", time.gmtime(self.clock()))
            self.status.log.append(f"{stamp} {line}")

    # --- session handlers --------------------------------------------------

    async def tool_started(self, name: str, tool_input: dict[str, Any]) -> None:
        if self.status is not None:
            self.status.tools += 1
            self.status.last = summarize_tool(name, tool_input)
            self.note(f"→ {name} {json.dumps(tool_input, ensure_ascii=False)[:2000]}")

    async def tool_finished(self, name: str, failed: bool) -> None:
        self.note(f"← {name}{' (failed)' if failed else ''}")

    async def permission(self, name: str, tool_input: dict[str, Any], reason: str | None) -> Permission:
        loop = asyncio.get_running_loop()
        reply_to = self.status_id
        if name == "AskUserQuestion":
            try:
                questions = approval.validate_questions(tool_input)
            except ValueError as error:
                return Permission(False, str(error))
            text = question_text(self.config.id, questions)
            message_id = await self.chat.ask(Outgoing(text, reply_to=reply_to), questions)
            pending = Pending(message_id, loop.create_future(), questions)
        else:
            request = approval_request(self.config.id, name, tool_input, reason)
            message_id = await self.chat.approve(Outgoing(request.content, request.files, reply_to))
            text = request.content
            pending = Pending(message_id, loop.create_future())
        self.pending[message_id] = pending
        if self.status is not None:
            self.status.pending += 1
        self.note(f"? {name}: waiting for an approver")
        minutes = int(self.config.approval_timeout // 60)
        try:
            outcome = await asyncio.wait_for(pending.future, self.config.approval_timeout)
        except TimeoutError:
            result, footer = Permission(False, f"No approver answered within {minutes} minutes."), "⌛ timed out"
        except Cancelled as error:
            result, footer = Permission(False, f"Denied: {error}."), f"⏹ {error}"
        else:
            by = pending.decided_by or "an approver"
            if outcome is Verdict.ALLOW:
                result, footer = Permission(True), f"✅ approved by {by}"
            elif outcome is Verdict.DENY:
                result, footer = Permission(False, f"Denied by {by}."), f"❌ denied by {by}"
            else:
                result = Permission(True, updated_input={**tool_input, "answers": outcome})
                footer = f"✅ answered by {by}"
        finally:
            self.pending.pop(message_id, None)
            if self.status is not None:
                self.status.pending -= 1
        self.note(f"{name}: {footer}")
        with contextlib.suppress(Exception):
            await self.chat.edit(message_id, f"{text}\n{footer}"[:2000])
        return result

    def unread(self) -> str:
        return f"\nunread: {len(self.buffer)}"

    async def tool_post(self, args: dict[str, Any]) -> str:
        roots = Roots((self.config.workdir, self.config.state), self.config.attachment_limit)
        try:
            outgoing = render_post(args, owner_id=self.config.roster.owner.discord_id,
                                   roots=roots, tokens=self.tokens)
        except PostError as error:
            raise ToolError(f"{error}{self.unread()}") from error
        already = self.breaker.tripped is not None
        if not self.breaker.post(self.clock()):
            if not already:
                await self.trip()
            raise ToolError("Rate limit reached; the bridge is now paused.")
        message_id = await self.chat.send(outgoing)
        if self.status is not None:
            self.status.posts += 1
        self.note(f"posted {args.get('kind')}: {args.get('headline')}")
        return f"posted as message {message_id}{self.unread()}"

    async def tool_history(self, args: dict[str, Any]) -> str:
        limit = max(1, min(int(args.get("limit") or 20), 100))
        messages = await self.chat.history(limit)
        heard = [m for m in messages if m.author.kind in (Kind.AGENT, Kind.SELF, Kind.WATCHDOG)
                 or (m.author.kind is Kind.HUMAN and m.author.role is not None)]
        lines = [f"[{m.id}] {m.author.label}: {m.content}" for m in heard]
        return "\n".join(lines) + self.unread()

    async def tool_rcon(self, args: dict[str, Any]) -> str:
        root = self.config.rcon_root
        if root is None:
            raise ToolError("RCON is not configured for this identity.")
        world, text = str(args.get("world", "")), " ".join(str(args.get("command", "")).split())
        text = text.removeprefix("/")
        if not text:
            raise ToolError("command is required")
        try:
            target = rcon.target(root, world)
        except rcon.RconError as error:
            raise ToolError(f"{error}{self.unread()}") from error
        # Read-only: always runs (and wins over the ask list). The ask list:
        # always a human. Anything else: in auto mode the classifier already
        # approved this call (rcon is not allow-listed there); otherwise a human.
        if rcon.matches(text, self.config.rcon_read_only):
            reason = None
        elif rcon.matches(text, self.config.rcon_ask):
            reason = "this console command always needs an approver"
        elif self.config.permission_mode == "auto":
            reason = None
        else:
            reason = "a console command that is not on the read-only list"
        if reason is not None:
            permission = await self.permission("rcon", {"world": world, "command": text}, reason)
            if not permission.allow:
                raise ToolError(f"{permission.message}{self.unread()}")
        try:
            reply = await rcon.command(target, text)
        except rcon.RconError as error:
            raise ToolError(f"{error}; `journalctl -u minecraft@{world}` shows whether it is up"
                            f"{self.unread()}") from error
        self.note(f"rcon {world}: {text}")
        return (reply or "(no reply)") + self.unread()

    async def tool_inbox(self, args: dict[str, Any]) -> str:
        entries, self.buffer = self.buffer, []
        lines = [e.line for e in entries]
        return ("\n".join(lines) if lines else "(no new messages)") + self.unread()
