"""Discord message rendering and `post` validation. Pure."""

from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path
import re
from typing import Any

from .filter import find_secret
from .policy import Incoming

MESSAGE_LIMIT = 2000
HEADLINE_LIMIT = 150
OVERVIEW_LIMIT = 1200
MAX_ATTACHMENTS = 10
KINDS = {"status": "💬", "question": "❓", "plan": "📋", "report": "📄", "alert": "🚨"}
NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}\Z")


class PostError(ValueError):
    """A post the agent must restructure; the message says how."""


@dataclass(frozen=True)
class File:
    name: str
    data: bytes


@dataclass(frozen=True)
class Outgoing:
    content: str
    files: tuple[File, ...] = ()
    reply_to: str | None = None
    mention_users: tuple[str, ...] = ()


@dataclass(frozen=True)
class Roots:
    """Where `path` attachments may come from."""

    paths: tuple[Path, ...]
    size_limit: int


def _inside(path: Path, roots: tuple[Path, ...]) -> bool:
    return any(path == root or root in path.parents for root in roots)


def _attachment(entry: Any, roots: Roots) -> File:
    if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
        raise PostError("each attachment needs a name and either content or path")
    name = entry["name"]
    if NAME.fullmatch(name) is None:
        raise PostError(f"attachment name {name!r} must be a plain file name like plan.md")
    if isinstance(entry.get("content"), str):
        data = entry["content"].encode()
    elif isinstance(entry.get("path"), str):
        path = Path(entry["path"]).resolve()
        resolved_roots = tuple(r.resolve() for r in roots.paths)
        if not _inside(path, resolved_roots):
            raise PostError(f"{entry['path']} is outside the workdir and state directory")
        if not path.is_file():
            raise PostError(f"{entry['path']} is not a file")
        if path.stat().st_size > roots.size_limit:
            raise PostError(f"{entry['path']} is larger than {roots.size_limit} bytes")
        data = path.read_bytes()
    else:
        raise PostError(f"attachment {name} needs content or path")
    if len(data) > roots.size_limit:
        raise PostError(f"attachment {name} is larger than {roots.size_limit} bytes")
    return File(name, data)


def render_post(args: dict[str, Any], *, owner_id: str, roots: Roots,
                tokens: tuple[str, ...] = ()) -> Outgoing:
    """Validate the `post` tool's arguments and render the message. Never truncates."""
    kind = args.get("kind")
    if kind not in KINDS:
        raise PostError(f"kind must be one of {', '.join(KINDS)}")
    headline = str(args.get("headline", "")).strip()
    overview = str(args.get("overview", "")).strip()
    if not headline:
        raise PostError("headline is required")
    if "\n" in headline or len(headline) > HEADLINE_LIMIT:
        raise PostError(f"headline must be one line of at most {HEADLINE_LIMIT} characters "
                        f"(it is {len(headline)}); move detail into the overview")
    if len(overview) > OVERVIEW_LIMIT:
        raise PostError(f"overview is {len(overview)} characters, over the {OVERVIEW_LIMIT} limit; "
                        "move the detail into an attachment and keep the overview to what "
                        "a busy admin needs")
    entries = args.get("attachments") or []
    if not isinstance(entries, list) or len(entries) > MAX_ATTACHMENTS:
        raise PostError(f"attachments must be a list of at most {MAX_ATTACHMENTS}")
    files = tuple(_attachment(entry, roots) for entry in entries)
    if kind == "plan" and not files:
        raise PostError("a plan needs at least one attachment holding the plan itself")
    if len({f.name for f in files}) != len(files):
        raise PostError("attachment names must be unique")
    mention = f"<@{owner_id}> " if kind == "alert" else ""
    content = f"{mention}{KINDS[kind]} **{headline}**" + (f"\n{overview}" if overview else "")
    if len(content) > MESSAGE_LIMIT:
        raise PostError("the message is too long; shorten the overview")
    for text in (content, *(f.data.decode("utf-8", "replace") for f in files)):
        secret = find_secret(text, tokens)
        if secret is not None:
            raise PostError(f"refused: the post appears to contain a secret ({secret})")
    reply_to = args.get("reply_to")
    return Outgoing(content, files, str(reply_to) if reply_to else None,
                    (owner_id,) if kind == "alert" else ())


@dataclass
class Status:
    """The live status message of one turn."""

    identity: str
    requester: str
    started: float
    tools: int = 0
    last: str = ""
    pending: int = 0
    posts: int = 0
    log: list[str] = field(default_factory=list)

    def render(self, now: float, final: str | None = None) -> str:
        elapsed = int(now - self.started)
        clock = f"{elapsed // 60:02d}:{elapsed % 60:02d}"
        head = {None: "⚙", "done": "✓", "error": "✗", "stopped": "⏹", "silent": "💤"}[final]
        lines = [f"{head} {self.identity} · working for {self.requester} · {clock}"]
        if self.last:
            lines.append(f"  last: {self.last}")
        counts = f"  {self.tools} tool call{'s' if self.tools != 1 else ''}"
        if self.pending:
            counts += f" · {self.pending} approval{'s' if self.pending != 1 else ''} pending"
        lines.append(counts)
        text = "\n".join(lines)
        return text if len(text) <= MESSAGE_LIMIT else text[:MESSAGE_LIMIT - 1] + "…"


def summarize_tool(name: str, tool_input: dict[str, Any]) -> str:
    """One line for the status message."""
    if name == "Bash":
        detail = str(tool_input.get("command", ""))
    elif "file_path" in tool_input:
        detail = str(tool_input["file_path"])
    elif "pattern" in tool_input:
        detail = str(tool_input["pattern"])
    elif name.startswith("mcp__bridge__"):
        return name.removeprefix("mcp__bridge__")
    else:
        detail = json.dumps(tool_input, ensure_ascii=False)
    detail = " ".join(detail.split())
    if len(detail) > 120:
        detail = detail[:119] + "…"
    return f"{name} `{detail.replace('`', 'ʼ')}`"


def approval_request(identity: str, name: str, tool_input: dict[str, Any],
                     reason: str | None) -> Outgoing:
    body = json.dumps(tool_input, indent=2, ensure_ascii=False)
    head = f"🔐 **{identity} asks to run {name}**"
    if reason:
        head += f"\n> {' '.join(reason.split())[:300]}"
    inline = f"{head}\n```json\n{body.replace('```', 'ʼʼʼ')}\n```"
    if len(inline) <= MESSAGE_LIMIT:
        return Outgoing(inline)
    return Outgoing(f"{head}\n(input attached)", (File("tool-input.json", body.encode()),))


def question_text(identity: str, questions: list[dict[str, Any]]) -> str:
    lines = [f"❓ **{identity} asks:**"]
    for index, question in enumerate(questions, 1):
        lines.append(f"**{index}. {question.get('question', '')}**")
        for option in question.get("options", []):
            description = option.get("description")
            lines.append(f"  • {option.get('label', '')}" + (f": {description}" if description else ""))
    lines.append("Only approvers can answer.")
    text = "\n".join(lines)
    return text if len(text) <= MESSAGE_LIMIT else text[:MESSAGE_LIMIT - 1] + "…"


def context_line(message: Incoming, trigger: bool, attachments: list[str]) -> str:
    """How a channel message appears in the agent's input."""
    where = f" in thread {message.thread_id}" if message.thread_id else ""
    marker = "may ask you to act" if trigger else "context only"
    line = f"[{message.id}{where}] {message.author.label}, {marker}: {message.content}"
    if attachments:
        line += "\n  attachments: " + ", ".join(attachments)
    return line
