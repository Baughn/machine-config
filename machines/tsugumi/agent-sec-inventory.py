"""Daily inventory for the security watch (agent-sec.nix): what tsugumi exposes, what runs
behind it, and vulnix's view of the system closure. Runs as root before the agent's turn;
the agent only reads the JSON it writes, and nothing the agent wrote is run here.

Usage: agent-sec-inventory OUT_DIR EXPOSED_JSON TRIGGER_DIR AGENT_USER
Environment: VULNIX_WHITELIST, VULNIX_CACHE, MC_ROOT, MC_WORLDS (space-separated),
NIXPKGS_PATH, NIXPKGS_VERSION.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import pwd
import re
import shutil
import subprocess
import sys
import time

STORE = re.compile(r"/nix/store/[0-9a-z]{32}-([^/]+)")


def store_name(path: str) -> str | None:
    match = STORE.match(path)
    return match.group(1) if match else None


def listening() -> list[dict[str, object]]:
    """Every listening TCP/UDP socket, with the processes behind it and their store paths."""
    output = subprocess.run(["ss", "-Hlntup"], capture_output=True, text=True, check=True).stdout
    sockets = []
    for line in output.splitlines():
        fields = line.split()
        if len(fields) < 5:
            continue
        proto, local = fields[0], fields[4]
        processes = []
        for name, pid in re.findall(r'\("([^"]*)",pid=(\d+)', line):
            try:
                exe = os.readlink(f"/proc/{pid}/exe")
            except OSError:
                exe = ""
            try:
                unit = Path(f"/proc/{pid}/cgroup").read_text().strip().rsplit("/", 1)[-1]
            except OSError:
                unit = ""
            processes.append({"name": name, "pid": int(pid), "exe": exe, "package": store_name(exe), "unit": unit})
        sockets.append({"proto": proto, "local": local, "processes": processes})
    return sockets


def vulnix() -> dict[str, object]:
    cache = os.environ["VULNIX_CACHE"]
    argv = ["vulnix", "--system", "--json", "--cache-dir", cache]
    if os.environ.get("VULNIX_WHITELIST"):
        argv += ["--whitelist", os.environ["VULNIX_WHITELIST"]]
    result = subprocess.run(argv, capture_output=True, text=True, timeout=3600)
    # vulnix exits 2 when it found something, 1 on errors (e.g. no network for the NVD feed).
    try:
        findings = json.loads(result.stdout) if result.stdout.strip() else []
    except json.JSONDecodeError:
        findings = []
    return {"exit": result.returncode, "findings": findings, "stderr": result.stderr[-4000:]}


def mods() -> dict[str, list[str]]:
    root = Path(os.environ.get("MC_ROOT", "/home/minecraft"))
    worlds = os.environ.get("MC_WORLDS", "").split()
    found = {}
    for world in worlds:
        directory = root / world / "mods"
        if directory.is_dir():
            found[world] = sorted(p.name for p in directory.iterdir() if p.suffix == ".jar")
    return found


def write(out: Path, name: str, data: object) -> None:
    tmp = out / f".{name}.tmp"
    tmp.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
    tmp.chmod(0o640)
    tmp.rename(out / name)


def main() -> None:
    out, exposed, triggers, user = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4]
    out.mkdir(parents=True, exist_ok=True)
    problems = []
    write(out, "system.json", {
        "generated": int(time.time()),
        "nixpkgs": os.environ.get("NIXPKGS_PATH"),
        "nixos_version": os.environ.get("NIXPKGS_VERSION"),
        "kernel": os.uname().release,
        "current_system": os.path.realpath("/run/current-system"),
        "booted_system": os.path.realpath("/run/booted-system"),
    })
    shutil.copyfile(exposed, out / ".exposed.tmp")
    (out / ".exposed.tmp").chmod(0o640)
    (out / ".exposed.tmp").rename(out / "exposed.json")
    for name, collect in (("listening.json", listening), ("mods.json", mods), ("vulnix.json", vulnix)):
        try:
            write(out, name, collect())
        except Exception as error:  # one failed collector shouldn't stop the others
            problems.append(f"{name}: {error}")
            write(out, name, {"error": str(error)})
    found = 0
    try:
        found = len(json.loads((out / "vulnix.json").read_text()).get("findings", []))
    except (OSError, ValueError, AttributeError):
        pass
    note = f"Inventory in {out}: system, exposed, listening, mods, vulnix ({found} vulnix findings)."
    if problems:
        note += " Collector problems: " + "; ".join(problems)
    account = pwd.getpwnam(user)
    stamp = time.strftime("%Y%m%dT%H%M%S")
    tmp = triggers / f"daily-{stamp}.tmp"
    tmp.write_text(json.dumps({"source": "daily", "note": note[:1900]}))
    os.chown(tmp, account.pw_uid, account.pw_gid)
    tmp.rename(triggers / f"daily-{stamp}.json")


if __name__ == "__main__":
    main()
