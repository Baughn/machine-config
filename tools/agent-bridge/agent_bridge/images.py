"""The image-size limit on Read. A large image makes a large message, and one
over the SDK's read buffer ends the turn (see MAX_BUFFER_SIZE in session.py)."""

from __future__ import annotations

from pathlib import Path
import struct
from typing import Any, BinaryIO

MAX_SIDE = 2048

_JPEG_SOF = {0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF}


def _jpeg_size(f: BinaryIO) -> tuple[int, int] | None:
    f.seek(2)
    while True:
        byte = f.read(1)
        while byte == b"\xff":
            byte = f.read(1)
        if not byte:
            return None
        marker = byte[0]
        if marker in (0x01, *range(0xD0, 0xD9)):  # no length field
            continue
        length = f.read(2)
        if len(length) < 2:
            return None
        if marker in _JPEG_SOF:
            data = f.read(5)
            if len(data) < 5:
                return None
            height, width = struct.unpack(">HH", data[1:5])
            return width, height
        f.seek(struct.unpack(">H", length)[0] - 2, 1)


def image_size(path: Path) -> tuple[int, int] | None:
    """(width, height) of a PNG, JPEG, GIF or WebP file; None for anything else."""
    try:
        with path.open("rb") as f:
            head = f.read(30)
            if head.startswith(b"\x89PNG\r\n\x1a\n") and len(head) >= 24:
                width, height = struct.unpack(">II", head[16:24])
                return width, height
            if head[:6] in (b"GIF87a", b"GIF89a") and len(head) >= 10:
                width, height = struct.unpack("<HH", head[6:10])
                return width, height
            if head[:4] == b"RIFF" and head[8:12] == b"WEBP" and len(head) >= 30:
                chunk = head[12:16]
                if chunk == b"VP8 ":
                    width, height = struct.unpack("<HH", head[26:30])
                    return width & 0x3FFF, height & 0x3FFF
                if chunk == b"VP8L":
                    bits = int.from_bytes(head[21:25], "little")
                    return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
                if chunk == b"VP8X":
                    return int.from_bytes(head[24:27], "little") + 1, int.from_bytes(head[27:30], "little") + 1
                return None
            if head.startswith(b"\xff\xd8"):
                return _jpeg_size(f)
    except (OSError, struct.error):
        pass
    return None


def oversized_read(tool_input: dict[str, Any], cwd: str | None) -> str | None:
    """Why a Read of this image is refused, or None to let it through."""
    file_path = tool_input.get("file_path")
    if not isinstance(file_path, str):
        return None
    path = Path(file_path)
    if not path.is_absolute() and cwd:
        path = Path(cwd) / path
    size = image_size(path)
    if size is None or max(size) <= MAX_SIDE:
        return None
    width, height = size
    return (f"{file_path} is {width}x{height}; images you read may be at most {MAX_SIDE}x{MAX_SIDE}. "
            f"Read a smaller copy, e.g. `magick {file_path} -resize '{MAX_SIDE}x{MAX_SIDE}>' small.png`.")
