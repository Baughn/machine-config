"""Outbound secret filter: a cheap backstop against accidents, not a guarantee."""

from __future__ import annotations

import re

PATTERNS = [
    ("private key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("age secret key", re.compile(r"AGE-SECRET-KEY-1[0-9A-Z]{20,}")),
    ("RCON password", re.compile(r"rcon\.password\s*=\s*\S")),
    ("Anthropic token", re.compile(r"sk-ant-[A-Za-z0-9_-]{20,}")),
    ("Discord token", re.compile(r"[MNO][A-Za-z0-9_-]{23,27}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,}")),
]


def find_secret(text: str, tokens: tuple[str, ...] = ()) -> str | None:
    """The kind of secret found in text, or None."""
    for token in tokens:
        if token and len(token) >= 8 and token in text:
            return "one of the bridge's own tokens"
    for name, pattern in PATTERNS:
        if pattern.search(text):
            return name
    return None
