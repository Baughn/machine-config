"""Shell commands reformatted for approvers to read, via shfmt."""

from __future__ import annotations

import subprocess

SHFMT = "shfmt"  # the Nix build substitutes the store path


def format_shell(command: str) -> str | None:
    """`command` with one statement per line, or None if shfmt can't parse it.
    shfmt prints the parsed syntax tree back, so the meaning is unchanged."""
    try:
        done = subprocess.run([SHFMT, "-i", "2"], input=command, capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return done.stdout.rstrip("\n") if done.returncode == 0 and done.stdout.strip() else None
