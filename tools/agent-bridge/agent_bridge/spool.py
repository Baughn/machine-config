"""Local triggers: programs running as the agent's own user drop JSON files into
$STATE/triggers/, and the bridge turns them into turns. Pure apart from the directory.

A file is `{"source": "<name>", "note": "<text>"}`. What the agent is asked to do comes
from the instance config (`trigger_sources`), never from the file: the note is data.
"""

from __future__ import annotations

from dataclasses import dataclass
import json
import logging
from pathlib import Path

from .schedule import NOTE_LIMIT

log = logging.getLogger(__name__)

FILE_LIMIT = 8192
PER_HOUR = 6  # spool turns; more files wait in the directory


@dataclass(frozen=True)
class Trigger:
    path: Path
    source: str
    note: str
    written: float  # the file's mtime


def parse(path: Path, sources: dict[str, str]) -> Trigger:
    """One spool file, or ValueError saying why it is rejected."""
    size = path.stat().st_size
    if size > FILE_LIMIT:
        raise ValueError(f"{size} bytes, over {FILE_LIMIT}")
    try:
        data = json.loads(path.read_bytes())
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"not JSON: {error}") from error
    if not isinstance(data, dict):
        raise ValueError("not a JSON object")
    source, note = data.get("source"), data.get("note", "")
    if not isinstance(source, str) or source not in sources:
        raise ValueError(f"unknown source {source!r}")
    if not isinstance(note, str):
        raise ValueError("note is not a string")
    if len(note) > NOTE_LIMIT:
        note = note[:NOTE_LIMIT] + " […truncated]"
    return Trigger(path, source, note.strip(), path.stat().st_mtime)


def scan(directory: Path, sources: dict[str, str], skip: set[Path]) -> list[Trigger]:
    """The directory's valid *.json files, oldest first, apart from those in `skip`.
    Invalid ones move to rejected/. Writers rename into place, so *.tmp is never read."""
    found = []
    try:
        paths = sorted(directory.glob("*.json"))
    except OSError:
        return []
    for path in paths:
        if path in skip or not path.is_file():
            continue
        try:
            found.append(parse(path, sources))
        except FileNotFoundError:
            continue
        except (OSError, ValueError) as error:
            log.warning("rejecting trigger file %s: %s", path.name, error)
            rejected = directory / "rejected"
            try:
                rejected.mkdir(exist_ok=True)
                path.replace(rejected / path.name)
            except OSError:
                log.exception("could not move %s aside", path.name)
    return sorted(found, key=lambda t: (t.written, t.path.name))
