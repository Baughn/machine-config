"""The agent session: a protocol, and its adapter onto the Claude Agent SDK."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
import json
import logging
import time
import warnings
from pathlib import Path
from typing import Any, Protocol

from .board import TOOL_NAMES as BOARD_TOOL_NAMES, TOOLS as BOARD_TOOLS
from .config import BASE_EFFORT
from .images import oversized_read

log = logging.getLogger(__name__)

# The SDK drops its connection to the CLI on any stdout line over this, and
# its default (1 MiB) is smaller than a tool result holding one large image.
MAX_BUFFER_SIZE = 32 * 1024 * 1024

# Session-ending reasons that mean the turn was interrupted.
INTERRUPTED = ("aborted_streaming", "aborted_tools")


@dataclass(frozen=True)
class TurnResult:
    session_id: str | None
    error: str | None = None
    interrupted: bool = False
    cost_usd: float | None = None
    cache: str | None = None  # prompt-cache token counts, for the turn log


LIMIT_NAMES = {"five_hour": "5-hour", "seven_day": "weekly", "seven_day_opus": "weekly Opus",
               "seven_day_sonnet": "weekly Sonnet", "overage": "overage"}


def turn_error(result: Any, limit: Any, assistant_error: str | None, text: str) -> str:
    """Why a turn failed, readably: a usage limit with its reset time, else what the CLI said."""
    if limit is not None or assistant_error == "rate_limit" or result.api_error_status == 429:
        what = "usage limit reached"
        if limit is not None and limit.rate_limit_type:
            what += f" ({LIMIT_NAMES.get(limit.rate_limit_type, limit.rate_limit_type)})"
        if limit is not None and limit.resets_at:
            what += time.strftime(", resets %Y-%m-%d %H:%M UTC", time.gmtime(limit.resets_at))
        elif text:
            what += f": {text}"
        return what
    error = "; ".join(result.errors or []) or text or result.result or assistant_error or result.subtype
    if result.api_error_status:
        error += f" (HTTP {result.api_error_status})"
    return " ".join(str(error).split())[:300]


def cache_usage(usage: dict[str, Any] | None) -> str | None:
    if not usage:
        return None
    keys = ("input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens")
    return ", ".join(f"{k.removesuffix('_tokens')} {usage.get(k, '?')}" for k in keys)


@dataclass(frozen=True)
class Permission:
    allow: bool
    message: str = ""
    updated_input: dict[str, Any] | None = None


class Handlers(Protocol):
    """What the session calls back into: implemented by the Bridge."""

    async def permission(self, name: str, tool_input: dict[str, Any], reason: str | None) -> Permission: ...
    async def tool_started(self, name: str, tool_input: dict[str, Any], subagent: bool = False) -> None: ...
    async def tool_finished(self, name: str, failed: bool, subagent: bool = False) -> str | None: ...
    async def tool_post(self, args: dict[str, Any]) -> str: ...
    async def tool_history(self, args: dict[str, Any]) -> str: ...
    async def tool_inbox(self, args: dict[str, Any]) -> str: ...
    async def tool_rcon(self, args: dict[str, Any]) -> str: ...
    async def tool_ask_agent(self, args: dict[str, Any]) -> str: ...
    async def tool_ship(self, args: dict[str, Any]) -> str: ...
    async def tool_schedule(self, args: dict[str, Any]) -> str: ...
    async def tool_schedules(self, args: dict[str, Any]) -> str: ...
    async def tool_effort(self, args: dict[str, Any]) -> str: ...
    async def tool_board(self, tool: str, args: dict[str, Any]) -> str: ...
    async def advisor_called(self) -> None: ...


class AgentSession(Protocol):
    async def connect(self, resume: str | None) -> None: ...
    async def turn(self, prompt: str) -> TurnResult: ...
    async def interrupt(self) -> None: ...
    async def disconnect(self) -> None: ...
    async def set_effort(self, level: str) -> None: ...


class ToolError(Exception):
    """Raised by bridge tools; reported to the model as a tool error."""


POST_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "kind": {"type": "string", "enum": ["status", "question", "plan", "report", "alert"],
                 "description": "alert mentions Baughn; plan needs an attachment holding the plan"},
        "headline": {"type": "string", "description": "One line, at most 150 characters"},
        "overview": {"type": "string", "description": "Markdown, at most 1200 characters"},
        "attachments": {
            "type": "array", "maxItems": 10,
            "items": {"type": "object", "required": ["name"], "properties": {
                "name": {"type": "string", "description": "File name, e.g. plan.md"},
                "content": {"type": "string"},
                "path": {"type": "string", "description": "A file inside your workdir or state directory"},
            }},
        },
        "reply_to": {"type": "string", "description": "Message id to reply to"},
        "thread": {"type": "string",
                   "description": "A Discord thread id, \"new: <title>\" to start a thread from this "
                                  "post, or \"main\". Default: the thread this turn started in"},
    },
    "required": ["kind", "headline"],
}


RCON_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "world": {"type": "string", "description": "World directory name, e.g. erisia"},
        "command": {"type": "string", "description": "Console command without a leading slash, e.g. list"},
    },
    "required": ["world", "command"],
}


def ask_schema(agents: tuple[str, ...]) -> dict[str, Any]:
    properties = {k: v for k, v in POST_SCHEMA["properties"].items() if k in ("headline", "overview", "attachments", "thread")}
    return {
        "type": "object",
        "properties": {
            "agent": {"type": "string", "enum": list(agents)},
            **properties,
            "timeout_minutes": {"type": "number", "minimum": 1, "maximum": 60,
                                "description": "How long to wait for the answer (default 30)"},
        },
        "required": ["agent", "headline"],
    }


SHIP_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {"bookmark": {"type": "string", "description": "Your bookmark on the commit, e.g. saya/fix-foo"}},
    "required": ["bookmark"],
}


SCHEDULE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "note": {"type": "string", "description": "What to do then, at most 2000 characters. It must stand "
                 "alone: by then you may be in a new session that remembers nothing else."},
        "in_minutes": {"type": "number", "minimum": 1, "description": "Fire this many minutes from now"},
        "at": {"type": "string", "description": "Or fire at this ISO 8601 time, e.g. 2026-09-27T18:00Z "
               "(UTC if it has no offset)"},
        "every_minutes": {"type": "number", "minimum": 15,
                          "description": "Repeat at this interval until cancelled; omit for once"},
        "thread": {"type": "integer", "description": "The board thread this follow-up belongs to"},
    },
    "required": ["note"],
}


def effort_schema(levels: tuple[str, ...]) -> dict[str, Any]:
    return {
        "type": "object",
        "properties": {
            "level": {"type": "string", "enum": list(levels)},
            "reason": {"type": "string", "description": "Why, in a few words; shown in the channel"},
        },
        "required": ["level", "reason"],
    }


SCHEDULES_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {"cancel": {"type": "string", "description": "A schedule id to cancel, e.g. s3"}},
}


def bridge_server(handlers: Handlers, rcon: bool, ask_agents: tuple[str, ...] = (), ship: bool = False,
                  effort_levels: tuple[str, ...] = (), board: bool = False) -> Any:
    from claude_agent_sdk import create_sdk_mcp_server, tool

    def wrap(function: Any) -> Any:
        async def call(args: dict[str, Any]) -> dict[str, Any]:
            try:
                text = await function(args)
            except ToolError as error:
                return {"content": [{"type": "text", "text": str(error)}], "is_error": True}
            return {"content": [{"type": "text", "text": text}]}
        return call

    tools = [
        tool("post", "Post a message in the Discord channel. This is the only way to say "
             "anything there; your final answer is never shown.", POST_SCHEMA)(wrap(handlers.tool_post)),
        tool("history", "Recent messages in the channel, or in one of its threads, oldest first, "
             "labelled with their authors.",
             {"type": "object", "properties": {
                 "limit": {"type": "integer", "minimum": 1, "maximum": 100},
                 "thread": {"type": "string", "description": "A thread id; default the main channel"}}})
        (wrap(handlers.tool_history)),
        tool("inbox", "Messages that arrived since your turn started (see `unread` in tool results). "
             "Reading them marks them delivered.", {"type": "object", "properties": {}})
        (wrap(handlers.tool_inbox)),
        tool("schedule", "Schedule a follow-up turn for yourself: at the given time the bridge starts a "
             "turn whose input is your note, even if nobody has said anything. Use it to check on "
             "something later instead of promising to. Schedules survive restarts and new sessions.",
             SCHEDULE_SCHEMA)(wrap(handlers.tool_schedule)),
        tool("schedules", "List your schedules, or cancel one.", SCHEDULES_SCHEMA)
        (wrap(handlers.tool_schedules)),
    ]
    if rcon:
        tools.append(tool("rcon", "Run a Minecraft server console command over RCON and return the "
                          "server's reply. Read-only commands (e.g. list) run at once; commands that "
                          "affect players or saving may need an approver.", RCON_SCHEMA)(wrap(handlers.tool_rcon)))
    if ask_agents:
        tools.append(tool("ask_agent", "Ask another agent in the channel something and wait for its "
                          "answer, which comes back as this tool's result. The question is posted "
                          "mentioning that agent; it answers with a reply to it. Later messages from it "
                          "arrive as channel context. If its bridge doesn't acknowledge the question "
                          "within 2 minutes (it is down, restarting or paused), the call returns at once "
                          "saying so.", ask_schema(ask_agents))(wrap(handlers.tool_ask_agent)))
    if ship:
        tools.append(tool("ship", "Ask Baughn to push a commit to master and deploy it. The deploy service "
                          "posts the diff for his approval, and pushes and deploys only after he approves. "
                          "Waits for the outcome and returns it.", SHIP_SCHEMA)(wrap(handlers.tool_ship)))
    if len(effort_levels) > 1:
        tools.append(tool("effort", f"Set how hard you think from your next step on: {BASE_EFFORT} or "
                          "higher. It holds for the rest of this turn; the next turn starts at "
                          f"{BASE_EFFORT} again. The level and reason are shown in the channel.",
                          effort_schema(effort_levels))(wrap(handlers.tool_effort)))
    if board:
        for name, (description, schema) in BOARD_TOOLS.items():
            async def call_board(args: dict[str, Any], name: str = name) -> str:
                return await handlers.tool_board(name, args)
            tools.append(tool(name, description, schema)(wrap(call_board)))
    return create_sdk_mcp_server("bridge", tools=tools)


MCP_TOOL_TIMEOUT_MS = 65 * 60 * 1000
RCON_TOOL = "mcp__bridge__rcon"
BRIDGE_TOOLS = ["mcp__bridge__post", "mcp__bridge__history", "mcp__bridge__inbox", RCON_TOOL,
                "mcp__bridge__ask_agent", "mcp__bridge__ship", "mcp__bridge__schedule",
                "mcp__bridge__schedules", "mcp__bridge__effort", *BOARD_TOOL_NAMES]
# Whatever the subagent is for, the main agent does the talking.
SUBAGENT_PROMPT = (
    "You are a subagent of {id}, working inside one of its turns. Report what you found or did in "
    "your final answer; {id} reads it and decides what to say in the Discord channel. Don't use "
    "the post, ask_agent, ship or schedule tools for your task. Post only if you find something "
    "Baughn must know that you think {id} might not pass on. This is NixOS: run a missing program "
    "with `nix-shell -p PKG --run 'CMD'`."
)
# The CLI's own schedulers and watchers would start the agent between turns,
# where nothing reads its output (see SdkSession.drain); the schedule tool replaces them.
CLI_SCHEDULERS = ["CronCreate", "CronDelete", "CronList", "ScheduleWakeup", "RemoteTrigger", "Monitor"]


def options_kwargs(*, workdir: Path, state: Path, cli_path: str | None, model: str | None,
                   resume: str | None, system_prompt: str, permission_mode: str,
                   allow: tuple[str, ...], ask: tuple[str, ...], deny: tuple[str, ...],
                   token: str, add_dirs: tuple[Path, ...] = (),
                   skills: tuple[str, ...] = (), identity: str = "the main agent",
                   advisor: str | None = None, auto_memory: bool = True) -> dict[str, Any]:
    """ClaudeAgentOptions arguments, apart from the callbacks. Pure, so it can be tested."""
    if permission_mode == "bypassPermissions":
        raise ValueError("bypassPermissions skips can_use_tool")
    kwargs: dict[str, Any] = dict(
        cwd=str(workdir),
        cli_path=cli_path,
        resume=resume,
        system_prompt={"type": "preset", "preset": "claude_code", "append": system_prompt},
        setting_sources=["project"],
        strict_mcp_config=True,
        permission_mode=permission_mode,
        # In auto mode rcon is left to the classifier, like Bash; see Bridge.tool_rcon.
        allowed_tools=[*(t for t in BRIDGE_TOOLS if not (permission_mode == "auto" and t == RCON_TOOL)),
                       *allow],
        disallowed_tools=[*deny, *CLI_SCHEDULERS],
        env={"CLAUDE_CODE_OAUTH_TOKEN": token, "CLAUDE_CONFIG_DIR": str(state / "claude"),
             # ask_agent holds its tool call open for up to an hour.
             "MCP_TOOL_TIMEOUT": str(MCP_TOOL_TIMEOUT_MS),
             # A background task's completion restarts the agent after its turn
             # has ended, where no turn reads its output (see SdkSession.drain).
             "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS": "1"},
        # Read-only commands run without asking only inside these and cwd.
        add_dirs=[str(d) for d in add_dirs],
        extra_args={"append-subagent-system-prompt": SUBAGENT_PROMPT.format(id=identity)},
    )
    if skills:
        # Pre-approves Skill(name) for these; discovery is from the project.
        kwargs["skills"] = list(skills)
    settings: dict[str, Any] = {}
    if ask:
        settings["permissions"] = {"ask": list(ask)}
    if advisor:
        # The server-side advisor tool: the model calls it, and the advisor
        # model reads the whole transcript. Still behind an opt-in in 2.1.x.
        settings["advisorModel"] = advisor
        kwargs["env"]["CLAUDE_CODE_ENABLE_EXPERIMENTAL_ADVISOR_TOOL"] = "1"
    if not auto_memory:
        # With the board, the CLI's private memory directory would be a third place to look.
        settings["autoMemoryEnabled"] = False
    if settings:
        kwargs["settings"] = json.dumps(settings)
    if model:
        kwargs["model"] = model
    return kwargs


class SdkSession:
    """One long-lived ClaudeSDKClient."""

    def __init__(self, handlers: Handlers, rcon: bool = False, ask_agents: tuple[str, ...] = (),
                 ship: bool = False, effort_levels: tuple[str, ...] = (), board: bool = False,
                 **kwargs: Any) -> None:
        self.handlers = handlers
        self.board = board
        self.rcon = rcon
        self.ask_agents = ask_agents
        self.ship = ship
        self.effort_levels = effort_levels
        self.kwargs = kwargs
        self.client: Any = None
        self.drainer: asyncio.Task[None] | None = None

    async def connect(self, resume: str | None) -> None:
        from claude_agent_sdk import (CanUseToolShadowedWarning, ClaudeAgentOptions, ClaudeSDKClient,
                                      HookMatcher, PermissionResultAllow, PermissionResultDeny)

        # The allow list is meant to skip can_use_tool; that is what it's for.
        warnings.filterwarnings("ignore", category=CanUseToolShadowedWarning)

        handlers = self.handlers

        async def can_use_tool(name: str, tool_input: dict[str, Any], context: Any) -> Any:
            reason = getattr(context, "decision_reason", None) or getattr(context, "title", None)
            result = await handlers.permission(name, tool_input, reason)
            if result.allow:
                return PermissionResultAllow(updated_input=result.updated_input)
            return PermissionResultDeny(message=result.message)

        async def pre(data: Any, tool_use_id: str | None, context: Any) -> Any:
            tool_input = data.get("tool_input") or {}
            await handlers.tool_started(data["tool_name"], tool_input, subagent=bool(data.get("agent_id")))
            refused = oversized_read(tool_input, data.get("cwd")) if data["tool_name"] == "Read" else None
            if refused:
                return {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                               "permissionDecisionReason": refused}}
            return {}

        async def post(data: Any, tool_use_id: str | None, context: Any) -> Any:
            event = data["hook_event_name"]
            news = await handlers.tool_finished(data["tool_name"], event != "PostToolUse",
                                                subagent=bool(data.get("agent_id")))
            if news and event == "PostToolUse":
                # Messages addressed to the agent, announced with whatever tool it ran.
                return {"hookSpecificOutput": {"hookEventName": event, "additionalContext": news}}
            return {}

        options = ClaudeAgentOptions(
            **options_kwargs(**{**self.kwargs, "resume": resume}),
            mcp_servers={"bridge": bridge_server(handlers, self.rcon, self.ask_agents, self.ship,
                                                 self.effort_levels, self.board)},
            can_use_tool=can_use_tool,
            hooks={"PreToolUse": [HookMatcher(hooks=[pre])],
                   "PostToolUse": [HookMatcher(hooks=[post])],
                   "PostToolUseFailure": [HookMatcher(hooks=[post])]},
            stderr=lambda line: log.info("claude: %s", line.rstrip()),
            max_buffer_size=MAX_BUFFER_SIZE,
        )
        self.client = ClaudeSDKClient(options)
        await self.client.connect()
        self.start_drain()

    async def turn(self, prompt: str) -> TurnResult:
        from claude_agent_sdk import AssistantMessage, RateLimitEvent, ResultMessage, ServerToolUseBlock, TextBlock

        await self.stop_drain()
        limit: Any = None  # the rate limit that refused us, if any
        assistant_error: str | None = None
        text = ""
        try:
            await self.client.query(prompt)
            async for message in self.client.receive_response():
                if isinstance(message, RateLimitEvent):
                    info = message.rate_limit_info
                    log.info("rate limit %s: %s, resets %s", info.rate_limit_type, info.status, info.resets_at)
                    limit = info if info.status == "rejected" else None
                if isinstance(message, AssistantMessage):
                    if any(isinstance(b, ServerToolUseBlock) and b.name == "advisor" for b in message.content):
                        await self.handlers.advisor_called()
                    if message.error:
                        assistant_error = message.error
                        text = " ".join(b.text for b in message.content if isinstance(b, TextBlock)).strip()
                if isinstance(message, ResultMessage):
                    interrupted = message.terminal_reason in INTERRUPTED
                    error = None
                    if message.is_error and not interrupted:
                        error = turn_error(message, limit, assistant_error, text)
                        log.warning("turn failed: %s", error)
                    return TurnResult(message.session_id, error, interrupted, message.total_cost_usd,
                                      cache_usage(message.usage))
            return TurnResult(None, "the session ended without a result")
        finally:
            self.start_drain()

    def start_drain(self) -> None:
        if self.client is not None and self.drainer is None:
            self.drainer = asyncio.create_task(self.drain(self.client))

    async def stop_drain(self) -> None:
        if self.drainer is not None:
            drainer, self.drainer = self.drainer, None
            drainer.cancel()
            try:
                await drainer
            except asyncio.CancelledError:
                pass

    async def drain(self, client: Any) -> None:
        """Read the CLI's output between turns.

        The SDK buffers only 100 messages. Once that fills, it stops reading the
        CLI altogether, so hook and permission callbacks go unanswered and every
        tool call hangs (2026-09-26: a background Bash task's completion restarted
        the lab agent after its turn). Nothing should arrive between turns; log it.
        """
        try:
            async for message in client.receive_messages():
                log.warning("CLI output outside a turn: %s", type(message).__name__)
        except asyncio.CancelledError:
            raise
        except Exception as error:
            log.warning("draining the CLI's output failed: %s", error)

    async def interrupt(self) -> None:
        if self.client is not None:
            await self.client.interrupt()

    async def set_effort(self, level: str) -> None:
        """Change effort mid-session. The SDK has no wrapper for this control request.

        The CLI sends a change like this as a per-message effort where the model
        and server allow it, which keeps the prompt cache.
        """
        if self.client is None:
            raise ToolError("no session")
        await self.client._query._send_control_request(
            {"subtype": "apply_flag_settings", "settings": {"effortLevel": level}})

    async def disconnect(self) -> None:
        await self.stop_drain()
        if self.client is not None:
            client, self.client = self.client, None
            await client.disconnect()
