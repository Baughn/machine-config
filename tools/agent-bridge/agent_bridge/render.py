"""Discord message rendering and `post` validation. Pure."""

from __future__ import annotations

from collections.abc import Callable
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


class ChatError(Exception):
    """Discord refused or couldn't place a message; the text says what to do about it."""


@dataclass(frozen=True)
class File:
    name: str
    data: bytes


MAIN = "main"
THREAD_TITLE_LIMIT = 100
SNOWFLAKE = re.compile(r"\d{15,21}\Z")


@dataclass(frozen=True)
class Outgoing:
    content: str
    files: tuple[File, ...] = ()
    reply_to: str | None = None
    mention_users: tuple[str, ...] = ()
    # Where it goes: a thread id of the channel, MAIN, or None (the thread of
    # reply_to, else the main channel).
    thread: str | None = None
    # Start a thread with this title from the message (sent in the main channel).
    new_thread: str | None = None
    # Carries a Stop button (the status message of a running turn).
    stop_button: bool = False


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


# A user or role mention. Posts never mention anyone (only alert, the owner).
MENTION = re.compile(r"<@[!&]?\d+>")


def parse_thread(value: Any) -> tuple[str | None, str | None]:
    """The `thread` argument: (thread, new_thread title). None means the default."""
    if value is None or value == "":
        return None, None
    text = str(value).strip()
    if text.lower() == MAIN:
        return MAIN, None
    if text.lower().startswith("new:"):
        title = " ".join(text[4:].split())
        if not title or len(title) > THREAD_TITLE_LIMIT:
            raise PostError(f"a new thread needs a title of 1-{THREAD_TITLE_LIMIT} characters: "
                            "thread \"new: <title>\"")
        if MENTION.search(title):
            raise PostError("a thread title can't contain a mention")
        return None, title
    if SNOWFLAKE.fullmatch(text) is None:
        raise PostError("thread must be a thread id, \"new: <title>\" or \"main\"")
    return text, None


# A bare @Name in a post, not inside code: `@` after a space, bracket or line start.
AT_NAME = re.compile(r"(?<![\w@<`/])@([A-Za-z][\w-]*)")
CODE = re.compile(r"```.*?```|`[^`\n]*`", re.DOTALL)


def link_mentions(text: str, agents: dict[str, str]) -> tuple[str, tuple[str, ...]]:
    """Turn bare @Name (an agent's display name or id, any case) into a real mention.

    Agents kept writing "@AdventurAgent, retry now" in posts, which reached nobody (Baughn,
    msg 1563905193526038559). `agents` maps lowercased names to Discord ids: only the agents
    this one may ask. Code spans and unknown names are left alone. Returns the text and the
    ids it mentions."""
    found: list[str] = []

    def replace(match: re.Match[str]) -> str:
        discord_id = agents.get(match[1].lower())
        if discord_id is None:
            return match[0]
        if discord_id not in found:
            found.append(discord_id)
        return f"<@{discord_id}>"

    parts, last = [], 0
    for code in CODE.finditer(text):
        parts.append(AT_NAME.sub(replace, text[last:code.start()]))
        parts.append(code[0])
        last = code.end()
    parts.append(AT_NAME.sub(replace, text[last:]))
    return "".join(parts), tuple(found)


def render_post(args: dict[str, Any], *, owner_id: str, roots: Roots,
                tokens: tuple[str, ...] = (), agents: dict[str, str] | None = None) -> Outgoing:
    """Validate the `post` tool's arguments and render the message. Never truncates.

    `agents` (lowercased name or id -> Discord id) are those a bare @Name may mention."""
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
    if MENTION.search(f"{headline}\n{overview}"):
        raise PostError("refused: the post contains a Discord mention (<@…>). Posts are sent with "
                        "mentions disabled, so it would neither ping nor trigger anyone. To address "
                        "an agent, write @ and its name (e.g. @Saya): that becomes a real mention "
                        "and starts its turn if it takes requests from you. To wait for its answer, use ask_agent; to "
                        "reach Baughn urgently, use kind alert.")
    entries = args.get("attachments") or []
    if not isinstance(entries, list) or len(entries) > MAX_ATTACHMENTS:
        raise PostError(f"attachments must be a list of at most {MAX_ATTACHMENTS}")
    files = tuple(_attachment(entry, roots) for entry in entries)
    if kind == "plan" and not files:
        raise PostError("a plan needs at least one attachment holding the plan itself")
    if len({f.name for f in files}) != len(files):
        raise PostError("attachment names must be unique")
    headline, in_headline = link_mentions(headline, agents or {})
    overview, in_overview = link_mentions(overview, agents or {})
    mentioned = tuple(dict.fromkeys(in_headline + in_overview))
    mention = f"<@{owner_id}> " if kind == "alert" else ""
    content = f"{mention}{KINDS[kind]} **{headline}**" + (f"\n{overview}" if overview else "")
    if len(content) > MESSAGE_LIMIT:
        raise PostError("the message is too long; shorten the overview")
    thread, new_thread = parse_thread(args.get("thread"))
    reply_to = args.get("reply_to")
    if new_thread is not None and reply_to:
        raise PostError("a post that starts a thread can't also be a reply; drop reply_to")
    for text in (content, new_thread or "", *(f.data.decode("utf-8", "replace") for f in files)):
        secret = find_secret(text, tokens)
        if secret is not None:
            raise PostError(f"refused: the post appears to contain a secret ({secret})")
    return Outgoing(content, files, str(reply_to) if reply_to else None,
                    tuple(dict.fromkeys(((owner_id,) if kind == "alert" else ()) + mentioned)),
                    thread, new_thread)


STATUS_LINE = re.compile(r"[⚙✓✗⏹💤] \S+ · working for ")


def is_status(content: str) -> bool:
    """Whether a message is some bridge's live status message."""
    return STATUS_LINE.match(content) is not None


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
    effort: str | None = None  # "high (reason)" while raised
    advisor: int = 0  # advisor consultations
    error: str | None = None  # why the turn failed, shown on the final message
    log: list[str] = field(default_factory=list)

    def render(self, now: float, final: str | None = None) -> str:
        elapsed = int(now - self.started)
        clock = f"{elapsed // 60:02d}:{elapsed % 60:02d}"
        head = {None: "⚙", "done": "✓", "error": "✗", "stopped": "⏹", "silent": "💤"}[final]
        lines = [f"{head} {self.identity} · working for {self.requester} · {clock}"]
        if self.last:
            lines.append(f"  last: {self.last}")
        counts = f"  {self.tools} tool call{'s' if self.tools != 1 else ''}"
        if self.advisor:
            counts += f" · advisor ×{self.advisor}"
        if self.pending:
            counts += f" · {self.pending} approval{'s' if self.pending != 1 else ''} pending"
        lines.append(counts)
        if self.effort:
            lines.append(f"  effort: {self.effort}")
        if self.error:
            lines.append(f"  error: {self.error}")
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


def pretty_input(name: str, tool_input: dict[str, Any], command: str | None = None) -> str:
    """A tool's input as an approver reads it: short values on a line each, long
    or multi-line text (a shell command, file contents) in its own code block.
    `command` replaces a Bash command for display (reformatted by shfmt)."""
    lines: list[str] = []
    for key, value in tool_input.items():
        shell = name == "Bash" and key == "command"
        if shell and command is not None:
            lines.append(f"command (reformatted by shfmt; exact text attached):\n```sh\n{command.replace('```', 'ʼʼʼ')}\n```")
        elif isinstance(value, str) and ("\n" in value or len(value) > 80 or shell):
            language = "sh" if shell else ""
            lines.append(f"{key}:\n```{language}\n{value.replace('```', 'ʼʼʼ')}\n```")
        else:
            text = value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)
            line = f"{key}: `{text.replace('`', 'ʼ')}`"
            if key == "timeout" and name == "Bash" and isinstance(value, (int, float)):
                line += f" ({value / 60000:g} min)"
            lines.append(line)
    return "\n".join(lines)


def approval_request(identity: str, name: str, tool_input: dict[str, Any],
                     reason: str | None,
                     shell_format: Callable[[str], str | None] | None = None) -> Outgoing:
    head = f"🔐 **{identity} asks to run {name}**"
    if reason:
        head += f"\n> {' '.join(reason.split())[:300]}"
    body = json.dumps(tool_input, indent=2, ensure_ascii=False)
    exact = (File("tool-input.json", body.encode()),)
    command = tool_input.get("command")
    if name == "Bash" and isinstance(command, str) and shell_format is not None:
        formatted = shell_format(command)
        if formatted is not None and formatted != command.strip():
            inline = f"{head}\n{pretty_input(name, tool_input, formatted)}"
            if len(inline) <= MESSAGE_LIMIT:
                return Outgoing(inline, exact)
    inline = f"{head}\n{pretty_input(name, tool_input)}"
    if len(inline) <= MESSAGE_LIMIT:
        return Outgoing(inline)
    return Outgoing(f"{head}\n(input attached)", exact)


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


def answer_footer(questions: list[dict[str, Any]], answers: dict[str, Any],
                  answered_by: dict[int, str]) -> str:
    """How answered questions are recorded on the question message: who chose what."""
    lines = ["✅ answered:"]
    for index, question in enumerate(questions):
        chosen = answers.get(question.get("question", ""), [])
        chosen = chosen if isinstance(chosen, list) else [chosen]
        lines.append(f"**{index + 1}.** → {', '.join(str(c) for c in chosen)} "
                     f"({answered_by.get(index, 'an approver')})")
    return "\n".join(lines)


def settled(text: str, footer: str) -> str:
    """A request or question with its outcome appended; the outcome always fits."""
    room = MESSAGE_LIMIT - len(footer) - 1
    if len(text) > room:
        text = text[:max(0, room - 1)] + "…"
    return f"{text}\n{footer}"[:MESSAGE_LIMIT]


def context_line(message: Incoming, trigger: bool, attachments: list[str]) -> str:
    """How a channel message appears in the agent's input."""
    where = ""
    if message.thread_id:
        name = f" \"{message.thread_name}\"" if message.thread_name else ""
        where = f" in thread{name} {message.thread_id}"
    marker = "may ask you to act" if trigger else "context only"
    line = f"[{message.id}{where}] {message.author.label}{message.reply_note}, {marker}: {message.content}"
    if attachments:
        line += "\n  attachments: " + ", ".join(attachments)
    return line
