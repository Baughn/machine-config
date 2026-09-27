"""Schedules: the store, and the bridge firing them as turns."""

from __future__ import annotations

from pathlib import Path

import pytest

from agent_bridge.bridge import Bridge
from agent_bridge.schedule import MAX_SCHEDULES, ScheduleError, Schedules, parse_time
from agent_bridge.session import ToolError, TurnResult

from conftest import ALICE, OWNER, FakeSession, Harness

NOW = 1_000_000.0


def store(tmp_path: Path) -> Schedules:
    return Schedules(tmp_path / "schedules.json")


def test_once_fires_once_and_persists(tmp_path: Path) -> None:
    schedules = store(tmp_path)
    s = schedules.add({"note": "check the node", "in_minutes": 10}, NOW)
    assert s.id == "s1" and s.due == NOW + 600
    assert Schedules(tmp_path / "schedules.json").items == [s]
    assert schedules.pop_due(NOW + 599) == []
    assert schedules.pop_due(NOW + 600) == [s]
    assert schedules.items == [] and Schedules(tmp_path / "schedules.json").items == []
    # Ids are never reused.
    assert schedules.add({"note": "again", "in_minutes": 1}, NOW).id == "s2"


def test_repeats_skip_missed_occurrences(tmp_path: Path) -> None:
    schedules = store(tmp_path)
    schedules.add({"note": "poll", "in_minutes": 5, "every_minutes": 15}, NOW)
    # Down for an hour: it fires once, and its next time is still on the grid.
    assert len(schedules.pop_due(NOW + 300 + 3600 + 1)) == 1
    assert schedules.items[0].due == NOW + 300 + 3600 + 900
    assert schedules.pop_due(NOW + 300 + 3600 + 2) == []


def test_at_and_validation(tmp_path: Path) -> None:
    schedules = store(tmp_path)
    assert parse_time("1970-01-12T13:46:40") == NOW
    assert parse_time("1970-01-12T14:46:40+01:00") == NOW
    s = schedules.add({"note": "x", "at": "1970-01-12T14:00Z"}, NOW)
    assert s.due == NOW + 800
    for bad in ({"note": "x"}, {"note": "x", "in_minutes": 1, "at": "1970-01-13"},
                {"note": "", "in_minutes": 1}, {"note": "x", "at": "1970-01-01"},
                {"note": "x", "at": "yesterday"}, {"note": "x", "in_minutes": 60 * 24 * 31},
                {"note": "x", "in_minutes": 1, "every_minutes": 5}, {"note": "x" * 2001, "in_minutes": 1}):
        with pytest.raises(ScheduleError):
            schedules.add(bad, NOW)
    for _ in range(MAX_SCHEDULES - 1):
        schedules.add({"note": "x", "in_minutes": 1}, NOW)
    with pytest.raises(ScheduleError, match="cancel some"):
        schedules.add({"note": "x", "in_minutes": 1}, NOW)
    schedules.cancel("s1")
    with pytest.raises(ScheduleError):
        schedules.cancel("s1")


async def test_a_schedule_starts_a_turn(harness: Harness) -> None:
    h = harness
    result = await h.bridge.tool_schedule({"note": "check ranks get_permission", "in_minutes": 30})
    assert result.startswith("scheduled s1: next ") and "unread: 0" in result
    h.bridge.fire_schedules()
    assert h.bridge.buffer == []
    assert 0 < h.bridge.sleep_time() <= 60
    h.clock.now += 1800
    assert h.bridge.sleep_time() == 0
    h.bridge.fire_schedules()
    assert h.bridge.wake.is_set()
    await h.bridge.one_turn()
    prompt = h.session.prompts[0]
    assert "[schedule s1] your own follow-up, set 1970-01-12 13:46 UTC, may ask you to act: " \
           "check ranks get_permission" in prompt
    status = next(m for m in h.chat.sent.values() if "working for" in m.content)
    # Not a Discord message: the status message can't reply to it.
    assert status.reply_to is None and "working for its schedule s1" in status.content
    assert h.bridge.schedules.items == []


async def test_paused_schedules_wait(harness: Harness) -> None:
    h = harness
    await h.bridge.tool_schedule({"note": "later", "in_minutes": 1})
    await h.say(OWNER, "!pause tsugumi-minecraft")
    h.clock.now += 120
    assert h.bridge.sleep_time() == 60  # no busy loop while it waits
    h.bridge.fire_schedules()
    assert h.bridge.buffer == [] and len(h.bridge.schedules.items) == 1
    await h.say(OWNER, "!resume tsugumi-minecraft")
    h.bridge.fire_schedules()
    assert [e.scheduled for e in h.bridge.buffer] == [True]


async def test_schedules_tool_and_status(harness: Harness) -> None:
    h = harness
    await h.bridge.tool_schedule({"note": "every so often", "in_minutes": 20, "every_minutes": 60})
    listing = await h.bridge.tool_schedules({})
    assert "s1: next " in listing and "every 60 min: every so often" in listing
    await h.say(ALICE, "!status")
    assert "1 schedules" in h.chat.texts()[-1]
    await h.say(ALICE, "!status tsugumi-minecraft schedules")
    sent = list(h.chat.sent.values())[-1]
    assert b"every so often" in sent.files[0].data
    assert "s1" not in (await h.bridge.tool_schedules({"cancel": "s1"})).split("\n", 2)[-1]
    with pytest.raises(ToolError, match="no schedule s1"):
        await h.bridge.tool_schedules({"cancel": "s1"})


async def test_schedules_survive_a_restart(tmp_path: Path) -> None:
    h = Harness(tmp_path)
    await h.bridge.tool_schedule({"note": "after restart", "in_minutes": 5})
    again = Bridge(h.config, h.chat, FakeSession, clock=h.clock)
    assert [s.note for s in again.schedules.items] == ["after restart"]


async def test_scheduling_works_during_the_handoff(harness: Harness) -> None:
    async def script(session: FakeSession, prompt: str) -> TurnResult:
        await session.bridge.tool_schedule({"note": "look again", "in_minutes": 60})
        return TurnResult("session-1")

    harness.session.script = script
    await harness.say(ALICE, "@me hi", mention=True)
    await harness.bridge.one_turn()
    harness.clock.now += harness.config.idle_reset + 1
    await harness.bridge.maybe_rollover()
    assert len(harness.bridge.schedules.items) == 2
