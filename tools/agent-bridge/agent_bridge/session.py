"""The agent session: a protocol, and its adapter onto the Claude Agent SDK."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
import json
import logging
import warnings
from pathlib import Path
from typing import Any, Protocol

log = logging.getLogger(__name__)

# Session-ending reasons that mean the turn was interrupted.
INTERRUPTED = ("aborted_streaming", "aborted_tools")


@dataclass(frozen=True)
class TurnResult:
    session_id: str | None
    error: str | None = None
    interrupted: bool = False
    cost_usd: float | None = None


@dataclass(frozen=True)
class Permission:
    allow: bool
    message: str = ""
    updated_input: dict[str, Any] | None = None


class Handlers(Protocol):
    """What the session calls back into: implemented by the Bridge."""

    async def permission(self, name: str, tool_input: dict[str, Any], reason: str | None) -> Permission: ...
    async def tool_started(self, name: str, tool_input: dict[str, Any]) -> None: ...
    async def tool_finished(self, name: str, failed: bool) -> None: ...
    async def tool_post(self, args: dict[str, Any]) -> str: ...
    async def tool_history(self, args: dict[str, Any]) -> str: ...
    async def tool_inbox(self, args: dict[str, Any]) -> str: ...
    async def tool_rcon(self, args: dict[str, Any]) -> str: ...
    async def tool_ask_agent(self, args: dict[str, Any]) -> str: ...
    async def tool_ship(self, args: dict[str, Any]) -> str: ...


class AgentSession(Protocol):
    async def connect(self, resume: str | None) -> None: ...
    async def turn(self, prompt: str) -> TurnResult: ...
    async def interrupt(self) -> None: ...
    async def disconnect(self) -> None: ...


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
    properties = {k: v for k, v in POST_SCHEMA["properties"].items() if k in ("headline", "overview", "attachments")}
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


def bridge_server(handlers: Handlers, rcon: bool, ask_agents: tuple[str, ...] = (), ship: bool = False) -> Any:
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
        tool("history", "Recent channel messages, oldest first, labelled with their authors.",
             {"type": "object", "properties": {"limit": {"type": "integer", "minimum": 1, "maximum": 100}}})
        (wrap(handlers.tool_history)),
        tool("inbox", "Messages that arrived since your turn started (see `unread` in tool results). "
             "Reading them marks them delivered.", {"type": "object", "properties": {}})
        (wrap(handlers.tool_inbox)),
    ]
    if rcon:
        tools.append(tool("rcon", "Run a Minecraft server console command over RCON and return the "
                          "server's reply. Read-only commands (e.g. list) run at once; commands that "
                          "affect players or saving may need an approver.", RCON_SCHEMA)(wrap(handlers.tool_rcon)))
    if ask_agents:
        tools.append(tool("ask_agent", "Ask another agent in the channel something and wait for its "
                          "answer, which comes back as this tool's result. The question is posted "
                          "mentioning that agent; it answers with a reply to it. Later messages from it "
                          "arrive as channel context.", ask_schema(ask_agents))(wrap(handlers.tool_ask_agent)))
    if ship:
        tools.append(tool("ship", "Ask Baughn to push a commit to master and deploy it. The deploy service "
                          "posts the diff for his approval, and pushes and deploys only after he approves. "
                          "Waits for the outcome and returns it.", SHIP_SCHEMA)(wrap(handlers.tool_ship)))
    return create_sdk_mcp_server("bridge", tools=tools)


MCP_TOOL_TIMEOUT_MS = 65 * 60 * 1000
RCON_TOOL = "mcp__bridge__rcon"
BRIDGE_TOOLS = ["mcp__bridge__post", "mcp__bridge__history", "mcp__bridge__inbox", RCON_TOOL,
                "mcp__bridge__ask_agent", "mcp__bridge__ship"]


def options_kwargs(*, workdir: Path, state: Path, cli_path: str | None, model: str | None,
                   resume: str | None, system_prompt: str, permission_mode: str,
                   allow: tuple[str, ...], ask: tuple[str, ...], deny: tuple[str, ...],
                   token: str, add_dirs: tuple[Path, ...] = (),
                   skills: tuple[str, ...] = ()) -> dict[str, Any]:
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
        disallowed_tools=list(deny),
        env={"CLAUDE_CODE_OAUTH_TOKEN": token, "CLAUDE_CONFIG_DIR": str(state / "claude"),
             # ask_agent holds its tool call open for up to an hour.
             "MCP_TOOL_TIMEOUT": str(MCP_TOOL_TIMEOUT_MS),
             # A background task's completion restarts the agent after its turn
             # has ended, where no turn reads its output (see SdkSession.drain).
             "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS": "1"},
        # Read-only commands run without asking only inside these and cwd.
        add_dirs=[str(d) for d in add_dirs],
    )
    if skills:
        # Pre-approves Skill(name) for these; discovery is from the project.
        kwargs["skills"] = list(skills)
    if ask:
        kwargs["settings"] = json.dumps({"permissions": {"ask": list(ask)}})
    if model:
        kwargs["model"] = model
    return kwargs


class SdkSession:
    """One long-lived ClaudeSDKClient."""

    def __init__(self, handlers: Handlers, rcon: bool = False, ask_agents: tuple[str, ...] = (),
                 ship: bool = False, **kwargs: Any) -> None:
        self.handlers = handlers
        self.rcon = rcon
        self.ask_agents = ask_agents
        self.ship = ship
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
            await handlers.tool_started(data["tool_name"], data.get("tool_input") or {})
            return {}

        async def post(data: Any, tool_use_id: str | None, context: Any) -> Any:
            await handlers.tool_finished(data["tool_name"], data["hook_event_name"] != "PostToolUse")
            return {}

        options = ClaudeAgentOptions(
            **options_kwargs(**{**self.kwargs, "resume": resume}),
            mcp_servers={"bridge": bridge_server(handlers, self.rcon, self.ask_agents, self.ship)},
            can_use_tool=can_use_tool,
            hooks={"PreToolUse": [HookMatcher(hooks=[pre])],
                   "PostToolUse": [HookMatcher(hooks=[post])],
                   "PostToolUseFailure": [HookMatcher(hooks=[post])]},
            stderr=lambda line: log.info("claude: %s", line.rstrip()),
        )
        self.client = ClaudeSDKClient(options)
        await self.client.connect()
        self.start_drain()

    async def turn(self, prompt: str) -> TurnResult:
        from claude_agent_sdk import ResultMessage

        await self.stop_drain()
        try:
            await self.client.query(prompt)
            async for message in self.client.receive_response():
                if isinstance(message, ResultMessage):
                    interrupted = message.terminal_reason in INTERRUPTED
                    error = None
                    if message.is_error and not interrupted:
                        error = "; ".join(message.errors or []) or message.subtype
                    return TurnResult(message.session_id, error, interrupted, message.total_cost_usd)
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

    async def disconnect(self) -> None:
        await self.stop_drain()
        if self.client is not None:
            client, self.client = self.client, None
            await client.disconnect()
