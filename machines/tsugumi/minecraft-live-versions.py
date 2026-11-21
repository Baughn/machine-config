"""Write which commits Incognito's own mods run, for lab-merge's live-commit check.

LabAgent (msg 1573712107931828365): the merge queue refuses a landing that would drop a commit
live on Incognito, but lab users can't read the live jars. This reads each jar in the world's
mods directory (start.py refreshes it only at a server start, so it is what runs), takes
`commit=` from any `<name>-build.properties` at the jar's root (EA: eln, Stasis: stasis; ErisiAgent,
msg 1573712345279111289), and writes {"world", "updated", "commits": {name: sha}} atomically.
Commits are abbreviated (12 hex digits) as the builds record them.

Usage: minecraft-live-versions MODS_DIR WORLD OUT_FILE
"""

import json
import os
import re
import sys
import tempfile
import time
import zipfile
from pathlib import Path

PROPS = re.compile(r"([a-z0-9_-]+)-build\.properties")
COMMIT = re.compile(r"commit=([0-9a-f]{7,40})\s*$", re.M)


def commits(mods: Path) -> dict[str, str]:
    found: dict[str, str] = {}
    for jar in sorted(mods.glob("*.jar")):
        try:
            with zipfile.ZipFile(jar) as archive:
                for name in archive.namelist():
                    match = PROPS.fullmatch(name)
                    if not match:
                        continue
                    text = archive.read(name).decode("utf-8", "replace")
                    commit = COMMIT.search(text)
                    if commit:
                        found[match.group(1)] = commit.group(1)
        except (OSError, zipfile.BadZipFile) as error:
            print(f"{jar.name}: {error}", file=sys.stderr)
    return found


def main(mods: str, world: str, out: str) -> None:
    target = Path(out)
    data = {"world": world, "commits": commits(Path(mods))}
    try:
        old = json.loads(target.read_text())
        if {k: old.get(k) for k in data} == data:
            return  # unchanged: keep the old "updated"
    except (OSError, ValueError):
        pass
    data["updated"] = int(time.time())
    fd, tmp = tempfile.mkstemp(dir=target.parent, prefix=".versions.")
    with os.fdopen(fd, "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)
        f.write("\n")
    os.chmod(tmp, 0o644)
    os.replace(tmp, target)
    print(f"{world}: {data['commits']}")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
