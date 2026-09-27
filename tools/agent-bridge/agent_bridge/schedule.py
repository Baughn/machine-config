"""Follow-ups the agent schedules for itself: persisted, and fired as turns by the bridge."""

from __future__ import annotations

import contextlib
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import json
from pathlib import Path
from typing import Any

MAX_SCHEDULES = 20
MIN_EVERY = 15 * 60.0
MAX_AHEAD = 30 * 86400.0
NOTE_LIMIT = 2000


class ScheduleError(ValueError):
    pass


def stamp(when: float) -> str:
    return time_utc(when).strftime("%Y-%m-%d %H:%M UTC")


def time_utc(when: float) -> datetime:
    return datetime.fromtimestamp(when, timezone.utc)


def parse_time(text: str) -> float:
    """An ISO 8601 time; UTC if it has no offset."""
    try:
        when = datetime.fromisoformat(text.strip())
    except ValueError as error:
        raise ScheduleError(f"at: {error}") from error
    if when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    return when.timestamp()


def number(args: dict[str, Any], key: str) -> float | None:
    value = args.get(key)
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError) as error:
        raise ScheduleError(f"{key} must be a number") from error


@dataclass(frozen=True)
class Schedule:
    id: str
    note: str
    due: float
    every: float | None  # seconds between repeats; None: once
    created: float

    def describe(self, now: float) -> str:
        minutes = max(0, round((self.due - now) / 60))
        text = f"{self.id}: next {stamp(self.due)} (in {minutes} min)"
        if self.every is not None:
            text += f", every {self.every / 60:g} min"
        return f"{text}: {self.note}"


class Schedules:
    """The identity's schedules, saved in <state>/schedules.json on every change."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.items: list[Schedule] = []
        self.next_id = 1
        with contextlib.suppress(FileNotFoundError, ValueError, KeyError, TypeError):
            data = json.loads(path.read_text())
            self.items = [Schedule(**s) for s in data["schedules"]]
            self.next_id = int(data["next_id"])

    def save(self) -> None:
        temporary = self.path.with_suffix(".tmp")
        temporary.write_text(json.dumps({"next_id": self.next_id,
                                         "schedules": [asdict(s) for s in self.items]}))
        temporary.replace(self.path)

    def add(self, args: dict[str, Any], now: float) -> Schedule:
        note = str(args.get("note") or "").strip()
        if not note:
            raise ScheduleError("note is required")
        if len(note) > NOTE_LIMIT:
            raise ScheduleError(f"note is over {NOTE_LIMIT} characters; put the details in a file and point to it")
        at, minutes = args.get("at"), number(args, "in_minutes")
        if (at is None) == (minutes is None):
            raise ScheduleError("give exactly one of in_minutes and at")
        due = parse_time(str(at)) if at is not None else now + (minutes or 0) * 60
        if due <= now:
            raise ScheduleError(f"{stamp(due)} is in the past; it is now {stamp(now)}")
        if due - now > MAX_AHEAD:
            raise ScheduleError(f"at most {MAX_AHEAD / 86400:g} days ahead")
        every = number(args, "every_minutes")
        if every is not None:
            every *= 60
            if every < MIN_EVERY:
                raise ScheduleError(f"every_minutes must be at least {MIN_EVERY / 60:g}")
        if len(self.items) >= MAX_SCHEDULES:
            raise ScheduleError(f"you already have {MAX_SCHEDULES} schedules; cancel some first")
        schedule = Schedule(f"s{self.next_id}", note, due, every, now)
        self.next_id += 1
        self.items.append(schedule)
        self.save()
        return schedule

    def cancel(self, schedule_id: str) -> Schedule:
        schedule = next((s for s in self.items if s.id == schedule_id), None)
        if schedule is None:
            raise ScheduleError(f"no schedule {schedule_id}")
        self.items.remove(schedule)
        self.save()
        return schedule

    def next_due(self) -> float | None:
        return min((s.due for s in self.items), default=None)

    def pop_due(self, now: float) -> list[Schedule]:
        """The schedules due by now. One-shots go; repeats move to their next future time,
        so repeats missed while the bridge was down or paused fire once, not once each."""
        due = [s for s in self.items if s.due <= now]
        if not due:
            return []
        kept = [s for s in self.items if s.due > now]
        for schedule in due:
            if schedule.every is not None:
                skipped = (now - schedule.due) // schedule.every + 1
                kept.append(Schedule(schedule.id, schedule.note, schedule.due + skipped * schedule.every,
                                     schedule.every, schedule.created))
        self.items = sorted(kept, key=lambda s: s.due)
        self.save()
        return due

    def listing(self, now: float) -> str:
        if not self.items:
            return "(no schedules)"
        return "\n".join(s.describe(now) for s in sorted(self.items, key=lambda s: s.due))
