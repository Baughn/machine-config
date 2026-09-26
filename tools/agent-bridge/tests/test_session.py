"""SdkSession against a fake ClaudeSDKClient: output between turns is drained."""

import asyncio
from typing import Any, AsyncIterator

from claude_agent_sdk import ResultMessage

from agent_bridge.session import SdkSession


def result() -> ResultMessage:
    return ResultMessage(subtype="success", duration_ms=1, duration_api_ms=1, is_error=False,
                         num_turns=1, session_id="s1")


class FakeClient:
    """Like the SDK: one bounded message stream, read by whoever iterates it."""

    def __init__(self) -> None:
        self.messages: asyncio.Queue[Any] = asyncio.Queue(maxsize=100)

    async def receive_messages(self) -> AsyncIterator[Any]:
        while True:
            yield await self.messages.get()

    async def receive_response(self) -> AsyncIterator[Any]:
        async for message in self.receive_messages():
            yield message
            if isinstance(message, ResultMessage):
                return

    async def query(self, prompt: str) -> None:
        await self.messages.put(object())
        await self.messages.put(result())

    async def disconnect(self) -> None:
        pass


async def test_output_between_turns_is_drained() -> None:
    session = SdkSession(handlers=None)  # type: ignore[arg-type]
    client = FakeClient()
    session.client = client
    session.start_drain()
    # More than the SDK's buffer, as a background task restarting the agent produced.
    for _ in range(250):
        await asyncio.wait_for(client.messages.put(object()), 1)
    turn = await asyncio.wait_for(session.turn("hello"), 1)
    assert turn.session_id == "s1" and turn.error is None
    assert session.drainer is not None  # draining again after the turn
    await session.disconnect()
    assert session.drainer is None
