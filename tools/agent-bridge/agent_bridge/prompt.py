"""System prompt assembly. Pure."""

from __future__ import annotations

from importlib import resources

from .config import Config


def base_prompt() -> str:
    return resources.files("agent_bridge").joinpath("base-prompt.md").read_text()


def roster_block(config: Config) -> str:
    roster = config.roster
    lines = ["## Roster", "", "Humans (only those currently holding the admin role are heard):"]
    for human in roster.humans:
        lines.append(f"- {human.name} (<@{human.discord_id}>), {human.role}")
    lines += ["", "Agents:"]
    for agent in roster.agents:
        me = " (you)" if agent.name == config.id else ""
        lines.append(f"- {agent.name}{me} (<@{agent.discord_id}>): {agent.description}")
    lines += [
        "",
        f"Who may ask you to act: {', '.join(sorted(config.triggers))}.",
        f"Who may approve your tool calls and answer your questions: {', '.join(sorted(config.approvers))}.",
    ]
    if config.owner_only:
        lines.append(f"You act only for {roster.owner.name}. Everyone else is context.")
    return "\n".join(lines)


def system_prompt(config: Config) -> str:
    base = base_prompt().replace("{id}", config.id)
    parts = [base, roster_block(config)]
    if config.prompt.strip():
        parts.append(config.prompt.strip())
    return "\n\n".join(parts)


def turn_prompt(lines: list[str]) -> str:
    body = "\n".join(lines) if lines else "(nothing new)"
    return (
        "Channel activity since your last turn, oldest first:\n\n"
        f"{body}\n\n"
        "Messages marked \"may ask you to act\" are addressed to you. To say anything in the "
        "channel, call the post tool; if you have nothing useful to add, end the turn without "
        "posting."
    )


HANDOFF_FILE = "notes/handoff.md"


def handoff_prompt(idle_hours: float, now: str) -> str:
    return (
        f"It is {now}. The channel has been quiet for {idle_hours:.1f} hours, so the bridge is about "
        "to end this session and start you a new one. The new session gets only its system prompt, "
        f"your files, and `{HANDOFF_FILE}`, which is included in its first turn. Nothing else from "
        "this conversation carries over.\n\n"
        "Before it ends, briefly:\n"
        "1. Update your other notes (`notes/`, `tools/README.md`) with anything durable from this "
        "session that isn't there yet.\n"
        f"2. Rewrite `{HANDOFF_FILE}` from scratch, in under 60 lines: work in flight, what you "
        "promised whom, what to check and when (absolute dates and times), decisions people made "
        "that aren't recorded elsewhere, and pointers into your other notes. Leave out whatever "
        "is finished and already recorded.\n\n"
        "Don't start any new investigation. Posting, inbox, rcon and approvals are switched off "
        "for this turn; messages that arrive meanwhile go to the new session."
    )


def new_session_preamble(handoff: str | None) -> str:
    if handoff is None:
        return (f"This is the first turn of a new session. There is no `{HANDOFF_FILE}`; your "
                "files in `notes/` and `tools/` are what you know from before.\n\n")
    return (f"This is the first turn of a new session. Your previous session left this in "
            f"`{HANDOFF_FILE}`:\n\n<handoff>\n{handoff}\n</handoff>\n\n")
