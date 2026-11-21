"""Local triggers: programs running as the agent's own user drop JSON files into
$STATE/triggers/, and the bridge turns them into turns. Pure apart from the directory.

A shared spool (`shared_spool`) also takes files from other users, for the sources
`trigger_writers` lists, each only from the users listed for it (the file's owner).

A file is `{"source": "<name>", "note": "<text>"}`. What the agent is asked to do comes
from the instance config (`trigger_sources`), never from the file: the note is data.
"""

from __future__ import annotations

from dataclasses import dataclass
import json
import logging
import os
from pathlib import Path
import pwd
import stat

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


def user_name(uid: int) -> str:
    try:
        return pwd.getpwuid(uid).pw_name
    except KeyError:
        return str(uid)


def read(path: Path) -> tuple[bytes, os.stat_result]:
    """A regular file's bytes and stat, never through a symlink (a shared spool's writers
    could plant one), with size and owner taken from the open file itself."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("not a regular file")
        if info.st_size > FILE_LIMIT:
            raise ValueError(f"{info.st_size} bytes, over {FILE_LIMIT}")
        data = os.read(fd, FILE_LIMIT + 1)
    finally:
        os.close(fd)
    if len(data) > FILE_LIMIT:
        raise ValueError(f"over {FILE_LIMIT} bytes")
    return data, info


def parse(path: Path, sources: dict[str, str],
          writers: dict[str, tuple[str, ...]] | None = None) -> Trigger:
    """One spool file, or ValueError saying why it is rejected. With `writers` (a shared
    spool), only those sources, each from a file its listed users own."""
    try:
        raw, info = read(path)
    except OSError as error:
        if isinstance(error, FileNotFoundError):
            raise
        raise ValueError(f"unreadable: {error}") from error
    try:
        data = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"not JSON: {error}") from error
    if not isinstance(data, dict):
        raise ValueError("not a JSON object")
    source, note = data.get("source"), data.get("note", "")
    if not isinstance(source, str) or source not in sources:
        raise ValueError(f"unknown source {source!r}")
    if writers is not None and (who := user_name(info.st_uid)) not in writers.get(source, ()):
        raise ValueError(f"{who} may not fire {source!r}")
    if not isinstance(note, str):
        raise ValueError("note is not a string")
    if len(note) > NOTE_LIMIT:
        note = note[:NOTE_LIMIT] + " […truncated]"
    return Trigger(path, source, note.strip(), info.st_mtime)


def scan(directory: Path, sources: dict[str, str], skip: set[Path],
         writers: dict[str, tuple[str, ...]] | None = None) -> list[Trigger]:
    """The directory's valid *.json files, oldest first, apart from those in `skip`.
    Invalid ones move to rejected/, or in a shared spool (`writers`) are deleted: its
    writers could plant `rejected` as a symlink (Garibaldi, board #15). Writers rename into
    place, so *.tmp is never read."""
    found = []
    try:
        paths = sorted(directory.glob("*.json"))
    except OSError:
        return []
    for path in paths:
        if path in skip:
            continue
        try:
            found.append(parse(path, sources, writers))
        except FileNotFoundError:
            continue
        except (OSError, ValueError) as error:
            log.warning("rejecting trigger file %s: %s", path.name, error)
            if writers is not None:
                try:
                    path.unlink(missing_ok=True)
                except OSError:
                    log.exception("could not delete %s", path.name)
                continue
            rejected = directory / "rejected"
            try:
                rejected.mkdir(exist_ok=True)
                path.replace(rejected / path.name)
            except OSError:
                log.exception("could not move %s aside", path.name)
    return sorted(found, key=lambda t: (t.written, t.path.name))
