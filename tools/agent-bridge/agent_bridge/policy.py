"""Routing: whether a Discord message starts a turn. Pure; the model never decides."""

from __future__ import annotations

from dataclasses import dataclass, field
import enum

from .config import Config

COMMANDS = ("stop", "pause", "resume", "reset", "status")


class Kind(enum.Enum):
    SELF = "self"
    AGENT = "agent"
    WATCHDOG = "watchdog"
    HUMAN = "human"
    OTHER = "other"  # unknown bots and webhooks


@dataclass(frozen=True)
class Author:
    id: str
    name: str
    kind: Kind
    role: str | None  # "owner", "admin", "agent" or None

    @property
    def label(self) -> str:
        role = self.role or ("watchdog" if self.kind is Kind.WATCHDOG else "no role")
        return f"{self.name} ({self.kind.value}, {role})"


@dataclass(frozen=True)
class Attachment:
    name: str
    url: str
    size: int


@dataclass(frozen=True)
class Incoming:
    id: str
    channel_id: str  # the channel, or the thread's parent channel
    thread_id: str | None
    author: Author
    content: str
    mentions: frozenset[str] = frozenset()
    role_mentions: frozenset[str] = frozenset()
    reply_to_author: str | None = None
    attachments: tuple[Attachment, ...] = field(default_factory=tuple)


class Route(enum.Enum):
    TRIGGER = "trigger"
    CONTEXT = "context"
    IGNORE = "ignore"


@dataclass(frozen=True)
class Decision:
    route: Route
    reason: str


@dataclass(frozen=True)
class Command:
    verb: str
    target: str | None  # an instance id, "all", or None
    argument: str | None = None


def classify(config: Config, author_id: str, name: str, *, is_bot: bool,
             webhook_id: str | None, is_admin: bool) -> Author:
    """Who wrote a message. `is_admin` is the live guild role check."""
    roster = config.roster
    if webhook_id is not None:
        if webhook_id == roster.watchdog_webhook_id:
            return Author(author_id, name, Kind.WATCHDOG, None)
        return Author(author_id, name, Kind.OTHER, None)
    if author_id == config.me.discord_id:
        return Author(author_id, config.id, Kind.SELF, "agent")
    agent = roster.agent(author_id)
    if agent is not None:
        return Author(author_id, agent.name, Kind.AGENT, "agent")
    if is_bot:
        return Author(author_id, name, Kind.OTHER, None)
    human = roster.human(author_id)
    # Removing someone's admin role in Discord revokes access, owner included.
    if not is_admin:
        return Author(author_id, human.name if human else name, Kind.HUMAN, None)
    if human is not None:
        return Author(author_id, human.name, Kind.HUMAN, human.role)
    return Author(author_id, name, Kind.HUMAN, "admin")


def mentioned(config: Config, message: Incoming) -> bool:
    me = config.me.discord_id
    return (me in message.mentions
            or message.reply_to_author == me
            or (config.roster.agents_role_id is not None
                and config.roster.agents_role_id in message.role_mentions))


def route(config: Config, message: Incoming, *, bot_streak: int, paused: bool) -> Decision:
    author = message.author
    if message.channel_id != config.channel_id:
        return Decision(Route.IGNORE, "outside the channel")
    if author.kind is Kind.SELF:
        return Decision(Route.IGNORE, "own message")
    if author.kind is Kind.OTHER:
        return Decision(Route.IGNORE, "unknown bot or webhook")
    if author.kind is Kind.HUMAN and author.role is None:
        return Decision(Route.IGNORE, "human without the admin role")
    if author.kind is Kind.WATCHDOG:
        return Decision(Route.CONTEXT, "watchdog")
    if not mentioned(config, message):
        return Decision(Route.CONTEXT, "not mentioned")
    if config.owner_only and author.role != "owner":
        return Decision(Route.CONTEXT, "owner-only identity")
    if author.role not in config.triggers:
        return Decision(Route.CONTEXT, f"{author.role} may not trigger")
    if paused:
        return Decision(Route.CONTEXT, "paused")
    if author.kind is Kind.AGENT and bot_streak >= config.limits.bot_streak:
        return Decision(Route.CONTEXT, "bot streak limit")
    return Decision(Route.TRIGGER, "mentioned by an allowed author")


def may_approve(config: Config, author: Author) -> bool:
    if author.kind is not Kind.HUMAN or author.role is None:
        return False
    if config.owner_only:
        return author.role == "owner"
    return author.role in config.approvers


def parse_command(content: str) -> Command | None:
    words = content.strip().split()
    if not words or not words[0].startswith("!") or words[0][1:] not in COMMANDS:
        return None
    verb = words[0][1:]
    target = words[1] if len(words) > 1 else None
    argument = words[2] if len(words) > 2 else None
    if len(words) > 3:
        return None
    return Command(verb, target, argument)


def command_applies(config: Config, command: Command) -> bool:
    if command.target is None:
        return command.verb == "status"
    return command.target == config.id or (command.target == "all" and command.verb != "reset")
