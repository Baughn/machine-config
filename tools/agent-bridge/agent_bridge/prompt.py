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
