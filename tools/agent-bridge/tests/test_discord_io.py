from __future__ import annotations

from types import SimpleNamespace

from agent_bridge.discord_io import mentioned_users


def test_a_bots_managed_role_counts_as_mentioning_the_bot() -> None:
    user = SimpleNamespace(id=1)
    bot_role = SimpleNamespace(id=50, tags=SimpleNamespace(bot_id=100))
    admin_role = SimpleNamespace(id=8, tags=None)
    plain_role = SimpleNamespace(id=9, tags=SimpleNamespace(bot_id=None))
    assert mentioned_users([user], [bot_role, admin_role, plain_role]) == frozenset({"1", "100"})
    assert mentioned_users([], [admin_role]) == frozenset()
