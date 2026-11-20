"""Discord message rendering and `post` validation. Pure."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
import difflib
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
    model: str | None = None  # "claude-… (reason)" while on the alternative model
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
        if self.model:
            lines.append(f"  model: {self.model}")
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


# The Obelisk of Changes' chatter (board #73): players read it, so a tool call becomes a few
# allowlisted words, never its raw input. The board sanitizes it again.
DIGEST_PROGRAMS = frozenset({
    "gradle", "gradlew", "git", "jj", "nix", "nix-shell", "nix-build", "cargo", "python", "python3",
    "pytest", "rg", "grep", "find", "ls", "cat", "head", "tail", "sed", "awk", "wc", "diff", "sort",
    "jq", "curl", "systemctl", "journalctl", "lab-merge", "minecraft-lab", "scratch", "java", "ffmpeg",
    "sox", "make", "rsync", "tar", "unzip", "du", "df", "ps", "free", "mvn", "javap", "sqlite3",
})
DIGEST_SUBCOMMANDS = {
    "git": {"status", "log", "diff", "commit", "push", "pull", "fetch", "rebase", "checkout", "switch",
            "branch", "merge", "show", "add", "worktree", "stash", "cherry-pick", "reset", "tag", "clone",
            "blame", "bisect"},
    "jj": {"st", "status", "log", "diff", "new", "describe", "commit", "squash", "rebase", "git",
           "bookmark", "show", "edit", "abandon", "split", "restore"},
    "nix": {"build", "develop", "run", "eval", "flake", "shell", "log", "search"},
    "cargo": {"build", "test", "clippy", "fmt", "run", "check"},
    "lab-merge": {"enqueue", "run", "status"},
    "minecraft-lab": {"list", "start", "stop", "clone", "destroy", "status", "logs"},
    "systemctl": {"status", "start", "stop", "restart", "show", "list-units", "is-active"},
}
GRADLE_TASK = re.compile(r":?[A-Za-z][A-Za-z0-9]*(?::[A-Za-z][A-Za-z0-9]*)*")
DIGEST_SUFFIXES = frozenset({
    ".java", ".kt", ".kts", ".gradle", ".rs", ".py", ".nix", ".md", ".json", ".json5", ".toml", ".ts",
    ".js", ".sh", ".yml", ".yaml", ".txt", ".png", ".ogg", ".wav", ".glsl", ".fsh", ".vsh", ".mcmeta",
    ".lang", ".snbt", ".nbt", ".csv", ".html", ".css", ".xml", ".mcfunction",
})
# Path segments whose files show no name: secrets, homes' dotfiles, system dirs, world data.
DIGEST_HIDDEN = re.compile(
    r"(^|/)(secrets?|\.ssh|\.gnupg|\.config|\.local|etc|run|proc|sys|tmp|credentials?|playerdata|stats"
    r"|advancements|region|entities|poi|DIM-?1|DIM1|data)(/|$)|token|passw|\.env|\.age$|key",
    re.IGNORECASE)


def _digest_file(path: str) -> str:
    name = Path(path).name
    if not name or DIGEST_HIDDEN.search(path) or Path(name).suffix.lower() not in DIGEST_SUFFIXES:
        return "a file"
    if find_secret(name) or not re.fullmatch(r"[A-Za-z0-9._+-]{1,60}", name):
        return "a file"
    return name


def _lines(text: Any) -> int:
    return len(str(text).splitlines()) if text else 0


def _digest_shell(command: str) -> str:
    """The program and, from a list, its subcommand or gradle tasks; else just "shell"."""
    for segment in re.split(r"&&|\|\||;|\||\n", command):
        words = segment.split()
        while words and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=\S*", words[0]):
            words = words[1:]  # FOO=bar cmd
        if not words or words[0] in ("cd", "export", "set", "echo", "true", "sudo", "time", "env"):
            continue
        program = Path(words[0]).name
        if program not in DIGEST_PROGRAMS:
            return "shell"
        args = [w for w in words[1:] if not w.startswith("-")]
        if program in ("gradle", "gradlew"):
            tasks = [a for a in args if GRADLE_TASK.fullmatch(a)][:2]
            return " ".join(["gradle", *tasks])
        allowed = DIGEST_SUBCOMMANDS.get(program, set())
        if args and args[0] in allowed:
            return f"{program} {args[0]}"
        return program
    return "shell"


def digest_tool(name: str, tool_input: dict[str, Any]) -> str:
    """A tool call as the obelisk shows it: a verb and at most a file name or subcommand."""
    if name in ("Read", "NotebookRead"):
        return f"read {_digest_file(str(tool_input.get('file_path', '')))}"
    if name == "Edit":
        added, removed = _lines(tool_input.get("new_string")), _lines(tool_input.get("old_string"))
        return f"edit {_digest_file(str(tool_input.get('file_path', '')))} +{added} −{removed}"
    if name == "MultiEdit":
        edits = tool_input.get("edits") or []
        added = sum(_lines(e.get("new_string")) for e in edits if isinstance(e, dict))
        removed = sum(_lines(e.get("old_string")) for e in edits if isinstance(e, dict))
        return f"edit {_digest_file(str(tool_input.get('file_path', '')))} +{added} −{removed}"
    if name == "Write":
        return f"write {_digest_file(str(tool_input.get('file_path', '')))} +{_lines(tool_input.get('content'))}"
    if name == "NotebookEdit":
        return f"edit {_digest_file(str(tool_input.get('notebook_path', '')))}"
    if name == "Bash":
        return _digest_shell(str(tool_input.get("command", "")))
    if name in ("Grep", "Glob"):
        return "search"
    if name in ("Agent", "Task"):
        return "subagent"
    if name in ("WebFetch", "WebSearch"):
        return "web"
    if name == "TodoWrite":
        return "plan"
    if name.startswith("mcp__"):
        tool = name.rsplit("__", 1)[-1]
        return tool if re.fullmatch(r"[a-z_]{1,30}", tool) else "tool"
    return name.lower() if re.fullmatch(r"[A-Za-z]{1,30}", name) else "tool"


# The obelisk's diff waterfall (Baughn, msg 1573365797915467938): Edit/Write as +/- lines, only
# for source files in the work repos (LabAssistant, msg 1573366294743621735). Notes, clones and
# world data can hold player names and coordinates, so they never show.
DIFF_REPOS = ("electricalage", "electrical-age", "eln", "ea", "stasis", "watershed", "lab-tools",
              "obelisk", "nixos", "machine-config")
DIFF_EXCLUDED = frozenset({"notes", "clones", "clone", "scratch", "free-research", "consolidation",
                           "secrets", "secret", "world", "saves", "logs", "run", "build", ".gradle",
                           "crash-reports", "inbox", "outbox", "handover", "tmp"})
DIFF_SUFFIXES = frozenset({".java", ".kt", ".kts", ".gradle", ".rs", ".py", ".nix", ".toml", ".ts",
                           ".js", ".sh", ".glsl", ".fsh", ".vsh", ".json", ".json5", ".mcmeta", ".css",
                           ".html", ".xml", ".yml", ".yaml"})
DIFF_HUNK_LINES = 200
DIFF_LINE = 160


def diff_path_allowed(path: str) -> bool:
    """Whether a file's diff may show on the obelisk: a source file inside a work repo."""
    parts = [part.lower() for part in Path(path).parts]
    if not parts or Path(path).suffix.lower() not in DIFF_SUFFIXES or DIGEST_HIDDEN.search(path):
        return False
    if any(part in DIFF_EXCLUDED for part in parts):
        return False
    return any(part == repo or part.startswith((repo + "-", repo + "_"))
               for part in parts[:-1] for repo in DIFF_REPOS)


def diff_lines(name: str, tool_input: dict[str, Any]) -> tuple[str, list[str]] | None:
    """(file basename, +/- lines) for an Edit/MultiEdit/Write the obelisk may show, else None."""
    path = str(tool_input.get("file_path", ""))
    if name not in ("Edit", "MultiEdit", "Write") or not diff_path_allowed(path):
        return None
    if name == "Write":
        pairs = [("", str(tool_input.get("content", "")))]
    elif name == "Edit":
        pairs = [(str(tool_input.get("old_string", "")), str(tool_input.get("new_string", "")))]
    else:
        pairs = [(str(e.get("old_string", "")), str(e.get("new_string", "")))
                 for e in tool_input.get("edits") or [] if isinstance(e, dict)]
    lines: list[str] = []
    for old, new in pairs:
        for line in difflib.unified_diff(old.splitlines(), new.splitlines(), n=0, lineterm=""):
            if line.startswith(("---", "+++", "@@")):
                continue
            line = line.expandtabs(4)[:DIFF_LINE]
            # The board refuses lines over 300 bytes.
            lines.append(line.encode()[:300].decode(errors="ignore"))
    if not lines:
        return None
    return Path(path).name, lines[:DIFF_HUNK_LINES]


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
