from __future__ import annotations

from pathlib import Path
import struct

import pytest

from agent_bridge.images import MAX_SIDE, image_size, oversized_read


def png(width: int, height: int) -> bytes:
    return b"\x89PNG\r\n\x1a\n" + struct.pack(">I", 13) + b"IHDR" + struct.pack(">II", width, height) + b"\x08\x02\0\0\0"


def jpeg(width: int, height: int) -> bytes:
    app0 = b"\xff\xe0" + struct.pack(">H", 16) + b"JFIF\0" + b"\0" * 9
    sof = b"\xff\xc2" + struct.pack(">HBHH", 17, 8, height, width) + b"\0" * 10
    return b"\xff\xd8" + app0 + sof + b"\xff\xd9"


def gif(width: int, height: int) -> bytes:
    return b"GIF89a" + struct.pack("<HH", width, height) + b"\0" * 20


def webp_vp8x(width: int, height: int) -> bytes:
    body = b"VP8X" + struct.pack("<I", 10) + b"\0" * 4 + (width - 1).to_bytes(3, "little") + (height - 1).to_bytes(3, "little")
    return b"RIFF" + struct.pack("<I", len(body) + 4) + b"WEBP" + body


def webp_vp8l(width: int, height: int) -> bytes:
    bits = (width - 1) | ((height - 1) << 14)
    body = b"VP8L" + struct.pack("<I", 5) + b"\x2f" + bits.to_bytes(4, "little") + b"\0" * 5
    return b"RIFF" + struct.pack("<I", len(body) + 4) + b"WEBP" + body


@pytest.mark.parametrize("make", [png, jpeg, gif, webp_vp8x, webp_vp8l])
def test_image_size(tmp_path: Path, make: object) -> None:
    path = tmp_path / "image"
    path.write_bytes(make(2400, 1600))  # type: ignore[operator]
    assert image_size(path) == (2400, 1600)


def test_not_an_image(tmp_path: Path) -> None:
    (tmp_path / "notes.md").write_text("# hello\n" * 10)
    (tmp_path / "short.png").write_bytes(b"\x89PNG")
    assert image_size(tmp_path / "notes.md") is None
    assert image_size(tmp_path / "short.png") is None
    assert image_size(tmp_path / "missing.png") is None


def test_oversized_read(tmp_path: Path) -> None:
    (tmp_path / "big.png").write_bytes(png(2400, 1600))
    (tmp_path / "tall.jpg").write_bytes(jpeg(1000, MAX_SIDE + 1))
    (tmp_path / "ok.png").write_bytes(png(MAX_SIDE, MAX_SIDE))
    reason = oversized_read({"file_path": str(tmp_path / "big.png")}, None)
    assert reason is not None and "2400x1600" in reason
    assert oversized_read({"file_path": "tall.jpg"}, str(tmp_path)) is not None
    assert oversized_read({"file_path": str(tmp_path / "ok.png")}, None) is None
    assert oversized_read({"file_path": str(tmp_path / "missing.png")}, None) is None
    assert oversized_read({}, None) is None
