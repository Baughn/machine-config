"""Loop and rate limits. Pure; callers pass the time in."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass, field

from .config import Limits
from .policy import Kind


@dataclass
class Streak:
    """Consecutive agent messages since the last human one, channel-wide."""

    count: int = 0

    def observe(self, kind: Kind) -> None:
        if kind is Kind.HUMAN:
            self.count = 0
        elif kind in (Kind.AGENT, Kind.SELF):
            self.count += 1

    @classmethod
    def from_history(cls, kinds_oldest_first: list[Kind]) -> Streak:
        streak = cls()
        for kind in kinds_oldest_first:
            streak.observe(kind)
        return streak


@dataclass
class Breaker:
    """Per-identity circuit breaker. Once tripped, only reset() clears it."""

    limits: Limits
    tripped: str | None = None
    _turns: deque[float] = field(default_factory=deque)
    _posts: deque[float] = field(default_factory=deque)

    @staticmethod
    def _count(events: deque[float], now: float, window: float) -> int:
        return sum(1 for t in events if now - t < window)

    def _prune(self, now: float) -> None:
        for events in (self._turns, self._posts):
            while events and now - events[0] >= 3600:
                events.popleft()

    def turn(self, now: float) -> bool:
        """Record a turn start. False (and tripped) if over the limit."""
        return self._record(self._turns, now, [(3600, self.limits.turns_per_hour, "turns per hour")])

    def post(self, now: float) -> bool:
        return self._record(self._posts, now, [(60, self.limits.posts_per_minute, "posts per minute"),
                                               (3600, self.limits.posts_per_hour, "posts per hour")])

    def _record(self, events: deque[float], now: float, windows: list[tuple[float, int, str]]) -> bool:
        if self.tripped:
            return False
        self._prune(now)
        for window, limit, name in windows:
            if self._count(events, now, window) >= limit:
                self.tripped = f"more than {limit} {name}"
                return False
        events.append(now)
        return True

    def reset(self) -> None:
        self.tripped = None
        self._turns.clear()
        self._posts.clear()
