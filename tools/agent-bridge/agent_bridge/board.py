"""The agent board (tools/agent-board): its client, the board_* tools, and their text output."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import aiohttp

from .render import PostError, Roots, _attachment

TIMEOUT = aiohttp.ClientTimeout(total=30)
ATTACHMENT_LIMIT = 1024 * 1024


class BoardError(Exception):
    """A board call that failed; the message is for the agent."""


@dataclass(frozen=True)
class BoardConfig:
    socket: Path | None = None  # tsugumi: the API socket; the caller is identified by uid
    url: str | None = None  # elsewhere: the TCP listener, with a bearer token
    token: str | None = None


def when(timestamp: int | None) -> str:
    if timestamp is None:
        return "?"
    return datetime.fromtimestamp(timestamp, timezone.utc).strftime("%Y-%m-%d %H:%MZ")


def unix(text: str, what: str) -> int:
    """An ISO 8601 date or time (UTC if it has no offset) as unix seconds."""
    try:
        moment = datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError as error:
        raise BoardError(f"{what} {text!r}: use an ISO 8601 date or time, e.g. 2026-10-02") from error
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=timezone.utc)
    return int(moment.timestamp())


class BoardClient:
    def __init__(self, config: BoardConfig) -> None:
        if (config.socket is None) == (config.url is None):
            raise ValueError("the board needs a socket or a url")
        self.config = config

    async def request(self, method: str, path: str, params: dict[str, Any] | None = None,
                      body: Any = None) -> Any:
        query = {k: str(v).lower() if isinstance(v, bool) else str(v)
                 for k, v in (params or {}).items() if v is not None}
        headers = {"Authorization": f"Bearer {self.config.token}"} if self.config.token else {}
        connector = aiohttp.UnixConnector(path=str(self.config.socket)) if self.config.socket else None
        base = "http://board" if self.config.socket else (self.config.url or "").rstrip("/")
        try:
            async with aiohttp.ClientSession(connector=connector, timeout=TIMEOUT) as session:
                async with session.request(method, base + path, params=query, json=body,
                                           headers=headers) as response:
                    status = response.status
                    data = await response.json(content_type=None)
        except (aiohttp.ClientError, asyncio.TimeoutError, OSError, ValueError) as error:
            raise BoardError(f"the board is unreachable ({type(error).__name__}: {error}); "
                             "carry on without it and mention it if it persists") from error
        if status != 200:
            message = data.get("error") if isinstance(data, dict) else None
            raise BoardError(f"board: {message or f'HTTP {status}'}")
        return data


# --- text output ---------------------------------------------------------------------------

def thread_line(thread: dict[str, Any]) -> str:
    tags = f" [{', '.join(thread['tags'])}]" if thread.get("tags") else ""
    extra = ""
    if thread.get("waiting_on"):
        ref = f" ({thread['waiting_ref']})" if thread.get("waiting_ref") else ""
        extra += f"; waiting on {thread['waiting_on']}{ref}"
    if thread.get("due"):
        extra += f"; due {thread['due']}"
    line = (f"#{thread['id']} {thread['title']}{tags} - {thread['status']}, owner {thread['owner']}, "
            f"updated {when(thread['updated'])}{extra}")
    first = thread.get("summary", "").strip().splitlines()
    return line + (f"\n    {first[0]}" if first else "")


def post_text(post: dict[str, Any]) -> str:
    head = f"--- post #{post['id']} by {post['author']}, {when(post['created'])}"
    if post.get("reply_to"):
        head += f", reply to #{post['reply_to']}"
    if post.get("ask"):
        head += f", asks {post['ask']}" + (" (answered)" if post.get("answered") else " (unanswered)")
    if post.get("superseded_by"):
        head += f", SUPERSEDED by #{post['superseded_by']}"
    lines = [head, post["body"]]
    if post.get("links"):
        lines.append("links: " + ", ".join(post["links"]))
    for file in post.get("attachments", []):
        lines.append(f"attachment {file['id']}: {file['name']} ({file['size']} bytes; board_attachment)")
    return "\n".join(lines)


def thread_text(view: dict[str, Any]) -> str:
    thread = view["thread"]
    lines = [thread_line({**thread, "summary": ""})]
    if thread.get("summary"):
        by = f" (by {thread['summary_author']}, {when(thread.get('summary_updated'))}; " \
             f"{thread['summary_revisions']} revisions)"
        lines += ["", "Summary" + by + ":", thread["summary"]]
    else:
        lines += ["", "No summary yet."]
    if view.get("earlier_posts"):
        lines += ["", f"({view['earlier_posts']} earlier posts not shown)"]
    lines += [""] + [post_text(post) for post in view["posts"]]
    return "\n".join(lines)


def search_text(hits: list[dict[str, Any]]) -> str:
    if not hits:
        return "no hits"
    lines = []
    for hit in hits:
        superseded = " (superseded)" if hit.get("superseded") else ""
        if hit["kind"] == "discord":
            where = f"discord {hit['id']}"
        elif hit["kind"] == "thread":
            where = f"thread #{hit['id']} {hit['thread_title']!r} (title/summary)"
        else:
            where = f"{hit['kind']} {hit['id']} in thread #{hit['thread']} {hit['thread_title']!r}"
        lines.append(f"{where} by {hit['author']}, {when(hit['created'])}{superseded}: "
                     f"{' '.join(hit['snippet'].split())}")
    return "\n".join(lines)


def discord_text(context: dict[str, Any]) -> str:
    lines = []
    for message in context["messages"]:
        mark = ">>> " if message["id"] == context["focus"] else ""
        where = f" in thread {message['thread']}" if message.get("thread") else ""
        reply = f", reply to {message['reply_to']}" if message.get("reply_to") else ""
        lines.append(f"{mark}[{message['id']}{where}] {message['author']} ({message['author_kind']}), "
                     f"{when(message['created'])}{reply}: {message['content']}")
        for file in message.get("attachments", []):
            hint = f" (board_attachment discord_message={message['id']} index={file['index']})" \
                if file["has_text"] else ""
            lines.append(f"    attachment: {file['name']}, {file['size']} bytes{hint}")
    return "\n".join(lines)


def briefing_text(briefing: dict[str, Any]) -> str:
    """The session-start summary; empty sections are left out."""
    sections = []
    if briefing["asks"]:
        sections.append("Questions addressed to you, unanswered (reply with board_post reply_to):\n" + "\n".join(
            f"- post #{a['post']} in #{a['thread']} {a['thread_title']!r}, from {a['author']}, "
            f"{when(a['created'])}: {a['first_line']}" for a in briefing["asks"]))
    for key, title in (("waiting_on_you", "Threads waiting on you"),
                       ("waiting_on_others", "Your threads waiting on someone else (don't re-ask)"),
                       ("due", "Open threads with a due date"),
                       ("changed", "Your threads whose summary someone else changed")):
        if briefing[key]:
            sections.append(f"{title}:\n" + "\n".join("- " + thread_line(t) for t in briefing[key]))
    if not sections:
        return "Board: nothing is waiting for you."
    since = f" (changes since {when(briefing['since'])})" if briefing.get("since") else ""
    return f"Board briefing{since}:\n\n" + "\n\n".join(sections)


# --- the tools -----------------------------------------------------------------------------

ATTACHMENTS_SCHEMA: dict[str, Any] = {
    "type": "array", "maxItems": 10,
    "items": {"type": "object", "required": ["name"], "properties": {
        "name": {"type": "string", "description": "File name, e.g. results.md"},
        "content": {"type": "string"},
        "path": {"type": "string", "description": "A text file inside your workdir or state directory"},
    }},
}

TOOLS: dict[str, tuple[str, dict[str, Any]]] = {
    "board_list": (
        "List board threads (one per work topic) with the first line of each summary.",
        {"type": "object", "properties": {
            "status": {"type": "string", "enum": ["open", "resolved", "parked", "all"],
                       "description": "Default open"},
            "tag": {"type": "string"},
            "owner": {"type": "string"},
            "waiting_on": {"type": "string"},
            "mine": {"type": "boolean", "description": "Only threads you own or posted in"},
            "updated_since": {"type": "string", "description": "ISO 8601 date or time"},
        }}),
    "board_read": (
        "Read a board thread: its current summary and its posts, oldest first.",
        {"type": "object", "properties": {
            "thread": {"type": "integer"},
            "since_post": {"type": "integer", "description": "Only posts after this id"},
            "limit": {"type": "integer", "minimum": 1, "description": "Newest posts to show (default 50)"},
        }, "required": ["thread"]}),
    "board_search": (
        "Full-text search of the board: thread titles and summaries, posts, attachments, and the "
        "Discord channel's whole history (kind discord). FTS5 syntax: words, \"phrases\", OR, NOT, prefix*.",
        {"type": "object", "properties": {
            "query": {"type": "string"},
            "kind": {"type": "string", "enum": ["thread", "post", "attachment", "discord"]},
            "author": {"type": "string", "description": "An agent id or a person's name"},
            "since": {"type": "string", "description": "ISO 8601 date or time"},
            "until": {"type": "string", "description": "ISO 8601 date or time"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 100},
        }, "required": ["query"]}),
    "board_post": (
        "Post on the board: in an existing thread, or in a new one (new_thread). Use it for findings, "
        "results and decisions others should be able to find; text attachments are kept and searchable.",
        {"type": "object", "properties": {
            "body": {"type": "string", "description": "Markdown"},
            "thread": {"type": "integer"},
            "new_thread": {"type": "object", "required": ["title"], "properties": {
                "title": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"},
                         "description": "Lowercase a-z, 0-9 and -; e.g. reference for durable facts"},
                "summary": {"type": "string", "description": "The thread's current state"},
                "due": {"type": "string", "description": "YYYY-MM-DD"},
                "waiting_on": {"type": "string"},
            }},
            "reply_to": {"type": "integer", "description": "A post id"},
            "ask": {"type": "string", "description": "Who this post asks a question (agent id or name); "
                    "it shows in their briefing until they reply to it"},
            "attachments": ATTACHMENTS_SCHEMA,
            "links": {"type": "array", "items": {"type": "string"},
                      "description": "Discord message ids, commits, URLs"},
            "supersedes": {"type": "array", "items": {"type": "integer"},
                           "description": "Earlier posts this corrects; they rank lower and are marked"},
        }, "required": ["body"]}),
    "board_summary": (
        "Update a thread's state. A new summary replaces the current one (every revision is kept); "
        "other fields change only if given. An empty string clears due or waiting_on.",
        {"type": "object", "properties": {
            "thread": {"type": "integer"},
            "summary": {"type": "string", "description": "State, decisions and who made them, owner, next step"},
            "status": {"type": "string", "enum": ["open", "resolved", "parked"]},
            "title": {"type": "string"},
            "owner": {"type": "string"},
            "tags": {"type": "array", "items": {"type": "string"}},
            "due": {"type": "string", "description": "YYYY-MM-DD"},
            "waiting_on": {"type": "string"},
            "waiting_ref": {"type": "string", "description": "The post or Discord message id of the question"},
        }, "required": ["thread"]}),
    "board_attachment": (
        "Read an attachment: a board post's (id) or an archived Discord message's (discord_message + index).",
        {"type": "object", "properties": {
            "id": {"type": "integer"},
            "discord_message": {"type": "string"},
            "index": {"type": "integer", "minimum": 0},
        }}),
    "board_discord": (
        "An archived Discord message with its neighbours in the channel or thread, e.g. to follow up a "
        "search hit or find a decision by its message id.",
        {"type": "object", "properties": {
            "message_id": {"type": "string"},
            "context": {"type": "integer", "minimum": 0, "maximum": 50, "description": "Default 5 each side"},
        }, "required": ["message_id"]}),
}
TOOL_NAMES = tuple(f"mcp__bridge__{name}" for name in TOOLS)


def _id(value: Any, what: str) -> int:
    try:
        return int(str(value))
    except ValueError as error:
        raise BoardError(f"{what} must be a number, not {value!r}") from error


class BoardTools:
    """The board_* tools for one agent."""

    def __init__(self, client: BoardClient, agent: str, roots: Roots) -> None:
        self.client = client
        self.agent = agent
        self.roots = Roots(roots.paths, min(roots.size_limit, ATTACHMENT_LIMIT))

    async def call(self, tool: str, args: dict[str, Any]) -> str:
        handler = getattr(self, tool, None)
        if tool not in TOOLS or handler is None:
            raise BoardError(f"unknown board tool {tool}")
        result: str = await handler(args)
        return result

    async def briefing(self) -> str:
        return briefing_text(await self.client.request("GET", "/briefing"))

    async def board_list(self, args: dict[str, Any]) -> str:
        params = {"status": args.get("status"), "tag": args.get("tag"), "owner": args.get("owner"),
                  "waiting_on": args.get("waiting_on"),
                  "involved": self.agent if args.get("mine") else None,
                  "updated_since": unix(args["updated_since"], "updated_since")
                  if args.get("updated_since") else None}
        threads = await self.client.request("GET", "/threads", params)
        return "\n".join(thread_line(t) for t in threads) if threads else "no threads"

    async def board_read(self, args: dict[str, Any]) -> str:
        thread = _id(args.get("thread"), "thread")
        params = {"since_post": args.get("since_post"), "limit": args.get("limit", 50)}
        return thread_text(await self.client.request("GET", f"/threads/{thread}", params))

    async def board_search(self, args: dict[str, Any]) -> str:
        params = {"q": args.get("query", ""), "kind": args.get("kind"), "author": args.get("author"),
                  "since": unix(args["since"], "since") if args.get("since") else None,
                  "until": unix(args["until"], "until") if args.get("until") else None,
                  "limit": args.get("limit")}
        return search_text(await self.client.request("GET", "/search", params))

    def attachments(self, entries: Any) -> list[dict[str, str]]:
        files = []
        for entry in entries or []:
            try:
                file = _attachment(entry, self.roots)
            except PostError as error:
                raise BoardError(str(error)) from error
            try:
                files.append({"name": file.name, "content": file.data.decode("utf-8")})
            except UnicodeDecodeError as error:
                raise BoardError(f"attachment {file.name} isn't UTF-8 text; the board keeps text only") from error
        return files

    async def board_post(self, args: dict[str, Any]) -> str:
        if ("thread" in args) == ("new_thread" in args):
            raise BoardError("give exactly one of thread and new_thread")
        post = {"body": args.get("body", ""), "reply_to": args.get("reply_to"), "ask": args.get("ask"),
                "links": [str(link) for link in args.get("links", [])],
                "attachments": self.attachments(args.get("attachments")),
                "supersedes": [_id(p, "supersedes") for p in args.get("supersedes", [])]}
        if "thread" in args:
            thread = _id(args["thread"], "thread")
            created = await self.client.request("POST", f"/threads/{thread}/posts", body=post)
            return f"posted #{created['post']} in thread #{thread}"
        new = args["new_thread"]
        if not isinstance(new, dict):
            raise BoardError("new_thread needs at least a title")
        created = await self.client.request("POST", "/threads", body={**new, "post": post})
        return f"opened thread #{created['thread']} with post #{created['post']}"

    async def board_summary(self, args: dict[str, Any]) -> str:
        thread = _id(args.get("thread"), "thread")
        fields = ("summary", "status", "title", "owner", "tags", "due", "waiting_on", "waiting_ref")
        update = {key: args[key] for key in fields if key in args}
        if not update:
            raise BoardError("nothing to change")
        return "updated: " + thread_line(await self.client.request("PATCH", f"/threads/{thread}", body=update))

    async def board_attachment(self, args: dict[str, Any]) -> str:
        if args.get("discord_message") is not None:
            message = _id(args["discord_message"], "discord_message")
            index = _id(args.get("index", 0), "index")
            file = await self.client.request("GET", f"/discord/{message}/attachments/{index}")
            return f"{file['name']}:\n{file['text']}"
        if args.get("id") is None:
            raise BoardError("give id, or discord_message and index")
        file = await self.client.request("GET", f"/attachments/{_id(args['id'], 'id')}")
        return f"{file['name']} (post #{file['post']}, thread #{file['thread']}):\n{file['content']}"

    async def board_discord(self, args: dict[str, Any]) -> str:
        message = _id(args.get("message_id"), "message_id")
        context = await self.client.request("GET", f"/discord/{message}", {"context": args.get("context", 5)})
        return discord_text(context)
