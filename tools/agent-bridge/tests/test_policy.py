from __future__ import annotations

import itertools

from hypothesis import given, strategies as st
import pytest

from agent_bridge.policy import Command, Kind, Route, command_applies, may_approve, parse_command, route

from conftest import ALICE, CAROL, CHANNEL, LAB, ME, OWNER, author, config_data, make_config, message

AUTHORS = {
    "owner": (OWNER, True),
    "admin": (ALICE, True),
    "unlisted admin": (CAROL, True),
    "owner without role": (OWNER, False),
    "human without role": (CAROL, False),
    "agent": (LAB, True),
    "self": (ME, True),
    "watchdog": ("watchdog", False),
    "stranger bot": ("stranger-bot", False),
}


def decide(config, who, *, mention=True, streak=0, paused=False, **kwargs):
    ident, admin = AUTHORS[who]
    return route(config, message(config, ident, mention=mention, admin=admin, **kwargs),
                 bot_streak=streak, paused=paused).route


def test_classification():
    config = make_config()
    assert author(config, OWNER).role == "owner"
    assert author(config, ALICE).role == "admin"
    assert author(config, CAROL).role == "admin"  # admin role, not in the roster
    assert author(config, OWNER, admin=False).role is None
    assert author(config, LAB).kind is Kind.AGENT
    assert author(config, ME).kind is Kind.SELF
    assert author(config, "watchdog").kind is Kind.WATCHDOG
    assert author(config, "stranger-bot").kind is Kind.OTHER


@pytest.mark.parametrize("who,mention,expected", [
    ("owner", True, Route.TRIGGER),
    ("owner", False, Route.CONTEXT),
    ("admin", True, Route.TRIGGER),
    ("unlisted admin", True, Route.TRIGGER),
    ("agent", True, Route.TRIGGER),
    ("agent", False, Route.CONTEXT),
    ("owner without role", True, Route.IGNORE),
    ("human without role", True, Route.IGNORE),
    ("self", True, Route.IGNORE),
    ("watchdog", True, Route.CONTEXT),
    ("watchdog", False, Route.CONTEXT),
    ("stranger bot", True, Route.IGNORE),
])
def test_routing_table(who, mention, expected):
    assert decide(make_config(), who, mention=mention) is expected


def test_mention_forms():
    config = make_config(roster={**config_data()["roster"], "agents_role_id": "77"})
    assert route(config, message(config, ALICE, reply_to_author=ME), bot_streak=0, paused=False).route is Route.TRIGGER
    assert route(config, message(config, ALICE, role_mentions=frozenset({"77"})),
                 bot_streak=0, paused=False).route is Route.TRIGGER
    assert route(config, message(config, ALICE, reply_to_author=LAB), bot_streak=0, paused=False).route is Route.CONTEXT


def test_other_channels_are_ignored():
    assert decide(make_config(), "owner", channel="999") is Route.IGNORE


def test_triggers_config_restricts_agents():
    config = make_config(triggers=["owner", "admin"])
    assert decide(config, "agent") is Route.CONTEXT
    assert decide(config, "admin") is Route.TRIGGER


def test_bot_streak_stops_agents_not_humans():
    config = make_config()
    assert decide(config, "agent", streak=29) is Route.TRIGGER
    assert decide(config, "agent", streak=30) is Route.CONTEXT
    assert decide(config, "admin", streak=500) is Route.TRIGGER


def test_paused_makes_triggers_context():
    assert decide(make_config(), "owner", paused=True) is Route.CONTEXT


def owner_only():
    return make_config(id="tsugumi-minecraft", owner_only=True, triggers=["owner"], approvers=["owner"])


def test_owner_only_table():
    config = owner_only()
    assert decide(config, "owner") is Route.TRIGGER
    for who in AUTHORS:
        if who != "owner":
            assert decide(config, who) is not Route.TRIGGER, who


@given(who=st.sampled_from(sorted(set(AUTHORS) - {"owner"})), mention=st.booleans(),
       streak=st.integers(0, 100), paused=st.booleans(), reply=st.sampled_from([None, ME, LAB, OWNER]))
def test_owner_only_never_triggers_for_anyone_else(who, mention, streak, paused, reply):
    config = owner_only()
    assert decide(config, who, mention=mention, streak=streak, paused=paused, reply_to_author=reply) is not Route.TRIGGER
    ident, admin = AUTHORS[who]
    assert not may_approve(config, author(config, ident, admin=admin))


@given(triggers=st.sets(st.sampled_from(["owner", "admin", "agent"])))
def test_owner_only_contradictions_refuse_to_load(triggers):
    from agent_bridge.config import ConfigError
    if triggers == {"owner"}:
        make_config(owner_only=True, triggers=sorted(triggers), approvers=["owner"])
    else:
        with pytest.raises(ConfigError):
            make_config(owner_only=True, triggers=sorted(triggers), approvers=["owner"])
    with pytest.raises(ConfigError):
        make_config(owner_only=True, triggers=["owner"], approvers=["owner", "admin"])


def test_approvers():
    config = make_config()
    assert may_approve(config, author(config, OWNER))
    assert may_approve(config, author(config, ALICE))
    assert not may_approve(config, author(config, ALICE, admin=False))
    assert not may_approve(config, author(config, LAB))
    assert not may_approve(config, author(config, ME))
    assert not may_approve(config, author(config, "watchdog"))
    assert not may_approve(make_config(approvers=["owner"]), author(config, ALICE))


def test_commands():
    assert parse_command("!stop tsugumi-minecraft") == Command("stop", "tsugumi-minecraft")
    assert parse_command("!status tsugumi-minecraft log") == Command("status", "tsugumi-minecraft", "log")
    assert parse_command("!status") == Command("status", None)
    assert parse_command("stop it") is None
    assert parse_command("!frobnicate all") is None
    assert parse_command("!stop a b c") is None
    config = make_config()
    assert command_applies(config, Command("stop", "all"))
    assert command_applies(config, Command("stop", "tsugumi-minecraft"))
    assert not command_applies(config, Command("stop", "tsugumi-lab"))
    assert not command_applies(config, Command("reset", "all"))
    assert not command_applies(config, Command("stop", None))
    assert command_applies(config, Command("status", None))


def test_exhaustive_routing_never_triggers_on_self_or_strangers():
    config = make_config()
    for who, mention, streak, paused in itertools.product(["self", "stranger bot", "human without role"],
                                                         [True, False], [0, 50], [True, False]):
        assert decide(config, who, mention=mention, streak=streak, paused=paused) is Route.IGNORE
    assert CHANNEL
