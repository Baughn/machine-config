"""A minimal RCON client for the `rcon` tool, plus its read-only check."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path
import re
import struct

WORLD = re.compile(r"[a-z0-9][a-z0-9-]{0,31}\Z")
LOGIN, COMMAND = 3, 2


class RconError(Exception):
    pass


@dataclass(frozen=True)
class Target:
    port: int
    password: str


def matches(command: str, entries: tuple[str, ...]) -> bool:
    """Whether a console command starts with the words of one of the entries."""
    words = command.split()
    return any(words[:len(entry.split())] == entry.split() for entry in entries if entry.split())


def target(root: Path, world: str) -> Target:
    """RCON port and password from <root>/<world>/server.properties."""
    if WORLD.fullmatch(world) is None:
        raise RconError(f"{world!r} is not a world name")
    path = root / world / "server.properties"
    try:
        lines = path.read_text().splitlines()
    except OSError as error:
        raise RconError(f"cannot read {path}: {error.strerror}") from error
    properties = dict(line.split("=", 1) for line in lines if "=" in line and not line.startswith("#"))
    if properties.get("enable-rcon", "").strip() != "true":
        raise RconError(f"RCON is not enabled for {world}")
    password = properties.get("rcon.password", "").strip()
    if not password:
        raise RconError(f"{world} has no RCON password")
    return Target(int(properties.get("rcon.port", "25575").strip()), password)


def packet(request_id: int, kind: int, body: str) -> bytes:
    payload = struct.pack("<ii", request_id, kind) + body.encode() + b"\0\0"
    return struct.pack("<i", len(payload)) + payload


async def read_packet(reader: asyncio.StreamReader) -> tuple[int, int, str]:
    (length,) = struct.unpack("<i", await reader.readexactly(4))
    if not 10 <= length <= 1 << 20:
        raise RconError(f"bad RCON packet length {length}")
    data = await reader.readexactly(length)
    request_id, kind = struct.unpack("<ii", data[:8])
    return request_id, kind, data[8:-2].decode("utf-8", "replace")


async def command(target: Target, text: str, timeout: float = 10.0) -> str:
    async def run() -> str:
        reader, writer = await asyncio.open_connection("127.0.0.1", target.port)
        try:
            writer.write(packet(1, LOGIN, target.password))
            await writer.drain()
            request_id, _, _ = await read_packet(reader)
            if request_id == -1:
                raise RconError("RCON rejected the password")
            writer.write(packet(2, COMMAND, text))
            await writer.drain()
            _, _, body = await read_packet(reader)
            return body
        finally:
            writer.close()

    try:
        return await asyncio.wait_for(run(), timeout)
    except (OSError, asyncio.IncompleteReadError) as error:
        raise RconError(f"RCON connection failed: {error}") from error
    except TimeoutError as error:
        raise RconError("RCON timed out") from error
