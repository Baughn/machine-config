from __future__ import annotations

import json
from pathlib import Path

import pytest

from agent_bridge.config import ConfigError, check_workdir_settings, load
from agent_bridge.prompt import system_prompt
from agent_bridge.session import BRIDGE_TOOLS, options_kwargs

from conftest import config_data, make_config


def test_roundtrip_from_toml(tmp_path: Path) -> None:
    prompt = tmp_path / "prompt.md"
    prompt.write_text("You operate the servers.")
    toml = f"""
id = "tsugumi-minecraft"
workdir = "/srv/agent"
channel_id = "50"
triggers = ["owner", "admin"]
approvers = ["owner"]
permission_mode = "auto"
ask = ["Bash(systemctl restart *)"]
deny = ["Bash(sudo *)"]
prompt_file = "{prompt}"
[limits]
bot_streak = 5
[roster]
guild_id = "7"
admin_role_id = "8"
[roster.humans.baughn]
discord_id = "1"
role = "owner"
[roster.agents.tsugumi-minecraft]
discord_id = "100"
description = "ops"
"""
    path = tmp_path / "config.toml"
    path.write_text(toml)
    config = load(path, tmp_path)
    assert config.permission_mode == "auto"
    assert config.limits.bot_streak == 5
    assert config.me.discord_id == "100"
    assert "You operate the servers." in system_prompt(config)
    assert "tsugumi-minecraft (you)" in system_prompt(config)


@pytest.mark.parametrize("overrides", [
    {"permission_mode": "bypassPermissions"},
    {"approvers": ["agent"]},
    {"triggers": ["everyone"]},
    {"id": "nobody"},
    {"workdir": str(Path.home())},
    {"owner_only": True},
])
def test_invalid_configs(overrides: dict[str, object]) -> None:
    with pytest.raises(ConfigError):
        make_config(**overrides)


def test_roster_needs_one_owner() -> None:
    roster = config_data()["roster"]
    roster["humans"]["alice"]["role"] = "owner"
    with pytest.raises(ConfigError):
        make_config(roster=roster)


def test_workdir_settings_guard(tmp_path: Path) -> None:
    (tmp_path / ".claude").mkdir()
    check_workdir_settings(tmp_path)
    (tmp_path / ".claude/settings.json").write_text(json.dumps({"env": {"X": "1"}}))
    check_workdir_settings(tmp_path)
    for body in ({"permissions": {"allow": ["Bash(*)"]}}, {"defaultMode": "bypassPermissions"}):
        (tmp_path / ".claude/settings.local.json").write_text(json.dumps(body))
        with pytest.raises(ConfigError):
            check_workdir_settings(tmp_path)


def kwargs(**overrides: object) -> dict[str, object]:
    base: dict[str, object] = dict(workdir=Path("/srv/agent"), state=Path("/var/lib/x"), cli_path="/bin/claude",
                                   model=None, resume=None, system_prompt="p", permission_mode="default",
                                   allow=("Read",), ask=(), deny=("Bash(sudo *)",), token="t")
    base.update(overrides)
    return options_kwargs(**base)  # type: ignore[arg-type]


def test_options_are_locked_down() -> None:
    options = kwargs()
    assert options["setting_sources"] == ["project"]
    assert options["env"]["CLAUDE_CONFIG_DIR"] == "/var/lib/x/claude"  # type: ignore[index]
    assert options["cwd"] == "/srv/agent"
    assert options["allowed_tools"] == [*BRIDGE_TOOLS, "Read"]
    assert options["disallowed_tools"] == ["Bash(sudo *)"]
    assert "settings" not in options
    with pytest.raises(ValueError):
        kwargs(permission_mode="bypassPermissions")


def test_skills_are_passed_but_sources_stay_project_only() -> None:
    options = kwargs(skills=("minecraft-tick-debug",))
    assert options["skills"] == ["minecraft-tick-debug"]
    assert options["setting_sources"] == ["project"]
    assert "skills" not in kwargs()


def test_ask_rules_go_in_flag_settings() -> None:
    options = kwargs(ask=("Bash(systemctl restart *)",))
    assert json.loads(str(options["settings"])) == {"permissions": {"ask": ["Bash(systemctl restart *)"]}}
