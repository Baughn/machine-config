"""agent-bridge run | selftest | contract-test."""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
from pathlib import Path
import sys

from .config import Config, check_workdir_settings, load

log = logging.getLogger("agent_bridge")


def credential(name: str) -> str:
    directory = os.environ.get("CREDENTIALS_DIRECTORY")
    if directory is None:
        sys.exit(f"agent-bridge: no CREDENTIALS_DIRECTORY; {name} comes from LoadCredential")
    return (Path(directory) / name).read_text().strip()


def load_config(args: argparse.Namespace) -> Config:
    state = Path(args.state or os.environ.get("STATE_DIRECTORY", "").split(":")[0] or ".")
    return load(Path(args.config), state)


def run(config: Config) -> None:
    from .bridge import Bridge
    from .discord_io import DiscordChat
    from .prompt import system_prompt
    from .session import SdkSession

    check_workdir_settings(config.workdir)
    if config.fake:
        log.info("fake mode: not connecting to Discord or Claude")
        asyncio.run(asyncio.Event().wait())
        return
    discord_token = credential("discord-token")
    claude_token = credential("claude-token")
    (config.state / "claude").mkdir(mode=0o700, exist_ok=True)
    chat = DiscordChat(config)
    prompt = system_prompt(config)

    def session(bridge: Bridge) -> SdkSession:
        return SdkSession(bridge, workdir=config.workdir, state=config.state, cli_path=config.cli_path,
                          model=config.model, resume=None, system_prompt=prompt,
                          permission_mode=config.permission_mode, allow=config.allow,
                          ask=config.ask, deny=config.deny, token=claude_token,
                          add_dirs=config.extra_dirs, skills=config.skills,
                          rcon=config.rcon_root is not None)

    chat.bridge = Bridge(config, chat, session, tokens=(discord_token, claude_token))
    chat.client.run(discord_token, log_handler=None)
    if chat.failed:
        sys.exit(1)


def selftest(config: Config) -> None:
    """Post one of each kind, a question and a status message to the configured channel."""
    from .discord_io import DiscordChat
    from .render import Outgoing, Roots, Status, approval_request, question_text, render_post

    chat = DiscordChat(config)
    roots = Roots((config.workdir, config.state), config.attachment_limit)
    owner = config.roster.owner.discord_id

    async def started() -> None:
        overview = "Rendering check: **bold**, `code`, and a [link](https://github.com/baughn/machine-config)."
        for kind in ("status", "question", "plan", "report", "alert"):
            args = {"kind": kind, "headline": f"selftest: a {kind} post", "overview": overview,
                    "attachments": [{"name": f"{kind}.md", "content": f"# {kind}\n\nAttachment body.\n"}]}
            await chat.send(render_post(args, owner_id=owner, roots=roots))
        questions = [{"question": "Which option renders best?", "header": "Render",
                      "options": [{"label": "First", "description": "The first choice"},
                                  {"label": "Second", "description": "The second choice"}],
                      "multiSelect": False},
                     {"question": "Which of these apply?", "header": "Multi",
                      "options": [{"label": "A"}, {"label": "B"}, {"label": "C"}], "multiSelect": True}]
        await chat.ask(Outgoing(question_text(config.id, questions)), questions)
        request = approval_request(config.id, "Bash", {"command": "systemctl restart minecraft@erisia",
                                                      "description": "Restart erisia"}, "selftest")
        await chat.approve(request)
        status = Status(config.id, "selftest", chat.client.loop.time() - 42)
        status.tools, status.last, status.pending = 12, "Bash `zfs list -t snapshot rpool/minecraft/erisia`", 1
        await chat.send(Outgoing(status.render(chat.client.loop.time())))
        log.info("selftest posted; answering the question shows the ephemeral reply. Ctrl-C to exit.")

    chat.started = started  # type: ignore[method-assign]
    chat.client.run(credential("discord-token"), log_handler=None)
    if chat.failed:
        sys.exit(1)


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="agent-bridge")
    parser.add_argument("command", choices=["run", "selftest", "contract-test"])
    parser.add_argument("--config", default=os.environ.get("AGENT_BRIDGE_CONFIG"))
    parser.add_argument("--state", help="state directory (default: $STATE_DIRECTORY)")
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s")
    logging.getLogger("discord").setLevel(logging.WARNING)
    if args.command == "contract-test":
        from .contract import main as contract
        sys.exit(asyncio.run(contract()))
    if not args.config:
        parser.error("--config or $AGENT_BRIDGE_CONFIG is required")
    config = load_config(args)
    if args.command == "run":
        run(config)
    else:
        selftest(config)


if __name__ == "__main__":
    main()
