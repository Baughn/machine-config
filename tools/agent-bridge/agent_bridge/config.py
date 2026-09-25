"""Instance config (TOML rendered by modules/agent-channel.nix) and its guards."""

from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path
import tomllib
from typing import Any

ROLES = ("owner", "admin", "agent")
MODES = ("default", "auto", "acceptEdits", "plan", "dontAsk")


class ConfigError(ValueError):
    pass


@dataclass(frozen=True)
class Human:
    name: str
    discord_id: str
    role: str  # "owner" or "admin"


@dataclass(frozen=True)
class Agent:
    name: str
    discord_id: str
    description: str


@dataclass(frozen=True)
class Roster:
    guild_id: str
    admin_role_id: str
    watchdog_webhook_id: str | None
    humans: tuple[Human, ...]
    agents: tuple[Agent, ...]
    agents_role_id: str | None = None

    @property
    def owner(self) -> Human:
        return next(h for h in self.humans if h.role == "owner")

    def human(self, discord_id: str) -> Human | None:
        return next((h for h in self.humans if h.discord_id == discord_id), None)

    def agent(self, discord_id: str) -> Agent | None:
        return next((a for a in self.agents if a.discord_id == discord_id), None)


@dataclass(frozen=True)
class Limits:
    bot_streak: int = 30
    turns_per_hour: int = 60
    posts_per_minute: int = 10
    posts_per_hour: int = 120


@dataclass(frozen=True)
class Config:
    id: str
    workdir: Path
    state: Path
    channel_id: str
    roster: Roster
    triggers: frozenset[str]
    approvers: frozenset[str]
    owner_only: bool = False
    permission_mode: str = "default"
    allow: tuple[str, ...] = ()
    ask: tuple[str, ...] = ()
    deny: tuple[str, ...] = ()
    prompt: str = ""
    cli_path: str | None = None
    model: str | None = None
    approval_timeout: float = 900.0
    attachment_limit: int = 8 * 1024 * 1024
    extra_dirs: tuple[Path, ...] = ()
    rcon_root: Path | None = None  # worlds live in <rcon_root>/<world>
    rcon_read_only: tuple[str, ...] = ()
    rcon_ask: tuple[str, ...] = ()  # always a human, even in auto mode
    skills: tuple[str, ...] = ()  # project skills in <workdir>/.claude/skills
    limits: Limits = field(default_factory=Limits)
    fake: bool = False

    @property
    def me(self) -> Agent:
        agent = next((a for a in self.roster.agents if a.name == self.id), None)
        if agent is None:
            raise ConfigError(f"{self.id} is not in the roster")
        return agent


def _strings(data: dict[str, Any], key: str) -> tuple[str, ...]:
    value = data.get(key, [])
    if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
        raise ConfigError(f"{key} must be a list of strings")
    return tuple(value)


def parse_roster(data: dict[str, Any]) -> Roster:
    humans = tuple(Human(name, str(h["discord_id"]), h["role"])
                   for name, h in data.get("humans", {}).items())
    agents = tuple(Agent(name, str(a["discord_id"]), a.get("description", ""))
                   for name, a in data.get("agents", {}).items())
    roster = Roster(
        guild_id=str(data["guild_id"]),
        admin_role_id=str(data["admin_role_id"]),
        watchdog_webhook_id=str(data["watchdog_webhook_id"]) if data.get("watchdog_webhook_id") else None,
        agents_role_id=str(data["agents_role_id"]) if data.get("agents_role_id") else None,
        humans=humans,
        agents=agents,
    )
    if [h.role for h in humans].count("owner") != 1:
        raise ConfigError("the roster needs exactly one owner")
    if any(h.role not in ("owner", "admin") for h in humans):
        raise ConfigError("human roles are owner or admin")
    ids = [h.discord_id for h in humans] + [a.discord_id for a in agents]
    if len(ids) != len(set(ids)):
        raise ConfigError("duplicate Discord IDs in the roster")
    return roster


def parse(data: dict[str, Any], state: Path) -> Config:
    limits = Limits(**data.get("limits", {}))
    config = Config(
        id=data["id"],
        workdir=Path(data["workdir"]),
        state=state,
        channel_id=str(data["channel_id"]),
        roster=parse_roster(data["roster"]),
        triggers=frozenset(_strings(data, "triggers")),
        approvers=frozenset(_strings(data, "approvers")),
        owner_only=bool(data.get("owner_only", False)),
        permission_mode=data.get("permission_mode", "default"),
        allow=_strings(data, "allow"),
        ask=_strings(data, "ask"),
        deny=_strings(data, "deny"),
        prompt=Path(data["prompt_file"]).read_text() if data.get("prompt_file") else "",
        cli_path=data.get("cli_path"),
        model=data.get("model"),
        approval_timeout=float(data.get("approval_timeout", 900)),
        limits=limits,
        fake=bool(data.get("fake", False)),
        extra_dirs=tuple(Path(d) for d in _strings(data, "extra_dirs")),
        rcon_root=Path(data["rcon_root"]) if data.get("rcon_root") else None,
        rcon_read_only=_strings(data, "rcon_read_only"),
        rcon_ask=_strings(data, "rcon_ask"),
        skills=_strings(data, "skills"),
    )
    validate(config)
    return config


def validate(config: Config) -> None:
    config.me  # noqa: B018 - raises if absent
    for group in (config.triggers, config.approvers):
        if not group <= set(ROLES):
            raise ConfigError(f"unknown roles: {sorted(group - set(ROLES))}")
    if "agent" in config.approvers:
        raise ConfigError("agents can never approve")
    if config.permission_mode not in MODES:
        # bypassPermissions approves before can_use_tool is consulted.
        raise ConfigError(f"permission mode {config.permission_mode!r} is not allowed")
    if config.owner_only and (config.triggers != {"owner"} or config.approvers != {"owner"}):
        raise ConfigError("owner_only requires triggers = approvers = [owner]")
    workdir = config.workdir.resolve()
    if workdir == Path.home().resolve() or workdir == Path("/"):
        raise ConfigError("the workdir must be a dedicated directory, never $HOME")


def load(path: Path, state: Path) -> Config:
    with path.open("rb") as stream:
        return parse(tomllib.load(stream), state)


def check_workdir_settings(workdir: Path) -> None:
    """Refuse project settings that would add permission rules: Nix is the single list."""
    for settings in sorted((workdir / ".claude").glob("settings*.json")):
        try:
            data = json.loads(settings.read_text())
        except json.JSONDecodeError as error:
            raise ConfigError(f"{settings} is not valid JSON: {error}") from error
        if not isinstance(data, dict):
            continue
        if "permissions" in data or "defaultMode" in data:
            raise ConfigError(f"{settings} sets permissions; permission rules live in the Nix config")
