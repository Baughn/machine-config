from __future__ import annotations

import json
from pathlib import Path

import pytest

from agent_bridge.config import ConfigError, check_workdir_settings, load
from agent_bridge.prompt import system_prompt
from agent_bridge.session import BRIDGE_TOOLS, CLI_SCHEDULERS, options_kwargs

from conftest import LAB, ME, author, config_data, make_config


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
    assert options["disallowed_tools"] == ["Bash(sudo *)", *CLI_SCHEDULERS]
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


def test_advisor_goes_in_flag_settings_with_its_opt_in() -> None:
    options = kwargs(advisor="claude-fable-5-1", ask=("Bash(x *)",))
    assert json.loads(str(options["settings"])) == {"permissions": {"ask": ["Bash(x *)"]},
                                                    "advisorModel": "claude-fable-5-1"}
    assert options["env"]["CLAUDE_CODE_ENABLE_EXPERIMENTAL_ADVISOR_TOOL"] == "1"  # type: ignore[index]
    assert "CLAUDE_CODE_ENABLE_EXPERIMENTAL_ADVISOR_TOOL" not in kwargs()["env"]  # type: ignore[operator]


def test_subagents_are_told_to_report_back() -> None:
    prompt = kwargs(identity="saya")["extra_args"]["append-subagent-system-prompt"]  # type: ignore[index]
    assert "subagent of saya" in prompt and "Don't use" in prompt


def test_effort_levels_and_prompt() -> None:
    assert make_config().effort_levels == ("medium",)
    config = make_config(max_effort="high", advisor="claude-fable-5-1")
    assert config.effort_levels == ("medium", "high")
    prompt = system_prompt(config)
    assert "Call `effort` with high" in prompt and "`advisor`" in prompt
    plain = system_prompt(make_config())
    assert "`effort`" not in plain and "`advisor`" not in plain
    with pytest.raises(ConfigError):
        make_config(max_effort="low")


def test_display_names() -> None:
    data = config_data()
    data["roster"]["agents"]["tsugumi-minecraft"]["display_name"] = "ErisiAgent"
    data["roster"]["agents"]["tsugumi-lab"]["display_name"] = "LabAgent"
    config = make_config(roster=data["roster"])
    prompt = system_prompt(config)
    assert "You are **ErisiAgent** (id `tsugumi-minecraft`)" in prompt
    assert "- ErisiAgent (you) (id `tsugumi-minecraft`)" in prompt
    assert "- LabAgent (id `tsugumi-lab`)" in prompt
    assert author(config, LAB).label == "LabAgent (agent, agent)"
    assert author(config, ME).label == "ErisiAgent (self, agent)"
