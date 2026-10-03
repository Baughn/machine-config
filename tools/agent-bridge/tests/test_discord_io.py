from __future__ import annotations

import asyncio
from types import SimpleNamespace
from typing import Any

import aiohttp
import discord
import pytest

from agent_bridge.discord_io import DiscordChat, mentioned_users
from agent_bridge.render import ChatError, Outgoing


def test_a_bots_managed_role_counts_as_mentioning_the_bot() -> None:
    user = SimpleNamespace(id=1)
    bot_role = SimpleNamespace(id=50, tags=SimpleNamespace(bot_id=100))
    admin_role = SimpleNamespace(id=8, tags=None)
    plain_role = SimpleNamespace(id=9, tags=SimpleNamespace(bot_id=None))
    assert mentioned_users([user], [bot_role, admin_role, plain_role]) == frozenset({"1", "100"})
    assert mentioned_users([], [admin_role]) == frozenset()


# --- threads -----------------------------------------------------------------

CHANNEL_ID = 50


def thread(thread_id: int, parent: int = CHANNEL_ID) -> Any:
    made = object.__new__(discord.Thread)
    made.id, made.parent_id, made.name, made.owner_id = thread_id, parent, "Dupers", 7
    return made


def chat(channels: dict[int, Any], fetched: dict[int, Any] | None = None) -> DiscordChat:
    async def fetch_channel(channel_id: int) -> Any:
        if fetched is None or channel_id not in fetched:
            raise discord.NotFound(SimpleNamespace(status=404, reason="nope"), "unknown")
        return fetched[channel_id]

    made = object.__new__(DiscordChat)
    made.channel = SimpleNamespace(id=CHANNEL_ID)
    made.where = {}
    made.owners = {}
    made.client = SimpleNamespace(get_channel=channels.get, fetch_channel=fetch_channel)
    return made


def test_posts_go_where_they_are_sent() -> None:
    live, archived = thread(60), thread(61)
    c = chat({60: live}, {61: archived})
    run = asyncio.run
    assert run(c.destination(Outgoing("x"))) is c.channel
    assert run(c.destination(Outgoing("x", thread="main"))) is c.channel
    assert run(c.destination(Outgoing("x", new_thread="T"))) is c.channel
    assert run(c.destination(Outgoing("x", thread="60"))) is live
    assert run(c.destination(Outgoing("x", thread="61"))) is archived  # fetched: not cached
    c.where[99] = 60
    assert run(c.destination(Outgoing("x", reply_to="99"))) is live


def test_only_this_channels_threads() -> None:
    c = chat({62: thread(62, parent=51), 63: SimpleNamespace(id=63)})
    for target in ("62", "63", "64"):
        with pytest.raises(ChatError):
            asyncio.run(c.destination(Outgoing("x", thread=target)))


def test_aiohttp_resolves_with_getaddrinfo() -> None:
    async def resolver() -> object:
        connector = aiohttp.TCPConnector()
        try:
            return connector._resolver
        finally:
            await connector.close()

    assert isinstance(asyncio.run(resolver()), aiohttp.ThreadedResolver)
