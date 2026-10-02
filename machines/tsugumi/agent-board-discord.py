"""agent-board-discord: copy the agent channel into the agent board's archive.

Polls Discord's REST API (no gateway) for the channel and its threads, cleans each
message up (mentions become names, embeds become text, small text attachments are
inlined) and posts it to the board over its unix socket, where this service's uid is
the `discord` identity. The board keeps the newest id per channel, so a restart picks
up where it left off and the first run backfills everything.
See machines/tsugumi/agent-board-discord.nix.

    agent-board-discord CONFIG.json

The bot token is the systemd credential `discord-token`.
"""

from datetime import datetime
import http.client
import json
import logging
import os
from pathlib import Path
import re
import socket
import sys
import time
import urllib.error
import urllib.request

API = "https://discord.com/api/v10"
USER_AGENT = "DiscordBot (https://github.com/baughn/machine-config, 1)"
DISCORD_EPOCH_MS = 1420070400000
MAX_TEXT = 1024 * 1024
TEXT_SUFFIXES = {".md", ".txt", ".diff", ".patch", ".log", ".json", ".toml", ".yaml", ".yml",
                 ".nix", ".py", ".rs", ".sh", ".csv", ".cfg", ".conf", ".java", ".zs", ".js"}
# Ordinary messages, replies and slash-command output; not joins, pins or thread notices.
CHAT_TYPES = {0, 19, 20}
ARCHIVE_SCAN_SECONDS = 3600
log = logging.getLogger("agent-board-discord")


def snowflake_time(snowflake):
    return ((int(snowflake) >> 22) + DISCORD_EPOCH_MS) // 1000


def iso_time(text):
    return int(datetime.fromisoformat(text).timestamp()) if text else None


def is_text(attachment):
    content_type = attachment.get("content_type") or ""
    suffix = Path(attachment.get("filename", "")).suffix.lower()
    looks_textual = (content_type.startswith("text/") or "json" in content_type
                     or suffix in TEXT_SUFFIXES)
    return looks_textual and attachment.get("size", MAX_TEXT + 1) <= MAX_TEXT


class Roster:
    """Who is who: roster names for known ids, the Discord username otherwise."""

    def __init__(self, config):
        self.names = config["names"]
        self.agents = set(config["agents"])

    def name(self, user):
        return self.names.get(user["id"]) or user.get("username") or user["id"]

    def kind(self, message):
        author = message["author"]
        if message.get("webhook_id"):
            return "webhook"
        if author["id"] in self.agents:
            return "approval" if message.get("content", "").startswith("\N{CLOSED LOCK WITH KEY}") else "agent"
        return "other" if author.get("bot") else "human"


def resolve_mentions(content, message, roster):
    users = {user["id"]: roster.name(user) for user in message.get("mentions", [])}
    content = re.sub(r"<@!?(\d+)>", lambda m: "@" + users.get(m[1], roster.names.get(m[1], m[1])),
                     content)
    content = re.sub(r"<@&(\d+)>", r"@role:\1", content)
    return re.sub(r"<#(\d+)>", r"#\1", content)


def embed_text(embed):
    parts = [embed.get("title"), embed.get("description")]
    parts += [f"{field.get('name', '')}: {field.get('value', '')}" for field in embed.get("fields", [])]
    parts.append((embed.get("footer") or {}).get("text"))
    return "\n".join(part for part in parts if part)


def convert(message, *, channel, thread, guild, roster, fetch_text):
    """A Discord API message as the board's archive wants it, or None to skip it."""
    if message.get("type", 0) not in CHAT_TYPES:
        return None
    content = resolve_mentions(message.get("content", ""), message, roster)
    for embed in message.get("embeds", []):
        text = embed_text(embed)
        if text:
            content += ("\n\n" if content else "") + resolve_mentions(text, message, roster)
    attachments = []
    for attachment in message.get("attachments", []):
        text = None
        if is_text(attachment):
            try:
                text = fetch_text(attachment["url"])
            except (OSError, urllib.error.URLError) as error:
                log.warning("attachment %s of %s: %s", attachment.get("filename"), message["id"], error)
        attachments.append({"name": attachment.get("filename", "?"),
                            "size": attachment.get("size", 0), "text": text})
    reference = message.get("message_reference") or {}
    return {
        "id": int(message["id"]),
        "channel": int(channel),
        "thread": int(thread) if thread else None,
        "author": roster.name(message["author"]),
        "author_kind": roster.kind(message),
        "created": snowflake_time(message["id"]),
        "edited": iso_time(message.get("edited_timestamp")),
        "reply_to": int(reference["message_id"]) if message.get("type") == 19 and reference.get("message_id") else None,
        "content": content,
        "attachments": attachments,
        "url": f"https://discord.com/channels/{guild}/{thread or channel}/{message['id']}",
    }


class UnixHTTPConnection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("board", timeout=60)
        self.path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.path)


class Board:
    def __init__(self, path):
        self.path = path

    def request(self, method, url, body=None):
        connection = UnixHTTPConnection(self.path)
        try:
            data = json.dumps(body).encode() if body is not None else None
            connection.request(method, url, body=data, headers={"content-type": "application/json"})
            response = connection.getresponse()
            payload = response.read()
            if response.status != 200:
                raise RuntimeError(f"board {method} {url}: {response.status} {payload[:300]!r}")
            return json.loads(payload)
        finally:
            connection.close()

    def cursor(self, channel, thread):
        query = f"/discord/cursor?channel={channel}" + (f"&thread={thread}" if thread else "")
        return self.request("GET", query)["last"]

    def ingest(self, messages):
        if messages:
            result = self.request("POST", "/discord", {"messages": messages})
            if result["inserted"] or result["updated"]:
                log.info("archived %(inserted)d new, %(updated)d edited", result)


class Discord:
    def __init__(self, token):
        self.token = token

    def get(self, path):
        while True:
            request = urllib.request.Request(API + path, headers={
                "Authorization": f"Bot {self.token}", "User-Agent": USER_AGENT})
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    if response.headers.get("X-RateLimit-Remaining") == "0":
                        time.sleep(float(response.headers.get("X-RateLimit-Reset-After", "1")))
                    return json.load(response)
            except urllib.error.HTTPError as error:
                if error.code == 401:
                    # Repeated invalid requests get the IP banned; wait for a new token.
                    log.error("Discord rejected the token; retrying in 10 minutes")
                    time.sleep(600)
                    continue
                if error.code != 429:
                    raise
                retry = json.load(error).get("retry_after", 5)
                log.info("rate limited for %ss", retry)
                time.sleep(float(retry))

    @staticmethod
    def fetch_text(url):
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read(MAX_TEXT + 1)[:MAX_TEXT].decode("utf-8", errors="replace")


class Poller:
    def __init__(self, config, discord, board):
        self.guild = config["guild"]
        self.channels = config["channels"]
        self.roster = Roster(config)
        self.discord = discord
        self.board = board
        self.archived_threads = {}  # thread id -> parent channel
        self.last_archive_scan = 0.0

    def convert_all(self, messages, channel, thread):
        converted = (convert(message, channel=channel, thread=thread, guild=self.guild,
                             roster=self.roster, fetch_text=self.discord.fetch_text)
                     for message in sorted(messages, key=lambda m: int(m["id"])))
        return [message for message in converted if message]

    def threads(self):
        """(parent channel, thread id) for active threads, plus archived ones hourly."""
        active = self.discord.get(f"/guilds/{self.guild}/threads/active")["threads"]
        found = {t["id"]: t["parent_id"] for t in active if t.get("parent_id") in self.channels}
        if time.monotonic() - self.last_archive_scan > ARCHIVE_SCAN_SECONDS:
            for channel in self.channels:
                before = ""
                while True:
                    page = self.discord.get(f"/channels/{channel}/threads/archived/public?limit=100{before}")
                    for t in page["threads"]:
                        self.archived_threads[t["id"]] = channel
                    if not page.get("has_more") or not page["threads"]:
                        break
                    before = "&before=" + page["threads"][-1]["thread_metadata"]["archive_timestamp"]
            self.last_archive_scan = time.monotonic()
            return found | self.archived_threads, found
        return found, found

    def catch_up(self, channel, thread):
        """Everything after the board's newest message here."""
        where = thread or channel
        after = self.board.cursor(channel, thread) or 0
        while True:
            page = self.discord.get(f"/channels/{where}/messages?after={after}&limit=100")
            self.board.ingest(self.convert_all(page, channel, thread))
            if len(page) < 100:
                return
            after = max(int(m["id"]) for m in page)

    def refresh_edits(self, channel, thread):
        """Recent messages edited since they were archived (e.g. status lines)."""
        page = self.discord.get(f"/channels/{thread or channel}/messages?limit=50")
        self.board.ingest(self.convert_all([m for m in page if m.get("edited_timestamp")],
                                           channel, thread))

    def cycle(self):
        for channel in self.channels:
            self.catch_up(channel, None)
            self.refresh_edits(channel, None)
        to_catch_up, active = self.threads()
        for thread, channel in to_catch_up.items():
            self.catch_up(channel, thread)
        for thread, channel in active.items():
            self.refresh_edits(channel, thread)


def main():
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    config = json.loads(Path(sys.argv[1]).read_text())
    token = (Path(os.environ["CREDENTIALS_DIRECTORY"]) / "discord-token").read_text().strip()
    poller = Poller(config, Discord(token), Board(config["socket"]))
    while True:
        try:
            poller.cycle()
        except Exception:  # noqa: BLE001 - keep polling through Discord or board outages
            log.exception("poll failed; retrying")
        time.sleep(config.get("interval", 30))


if __name__ == "__main__":
    main()
