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
# Output suffixes on store path names (openssl-3.4.1-bin): not part of the version.
OUTPUTS = re.compile(r"-(bin|dev|lib|out|man|doc|devdoc|info|debug|etc|dist|py|terminfo|static|"
                     r"modules|data|env|share|getent|mount|login|tools)$")
VERSIONED = re.compile(r"^(\S+?)-([0-9]\S*)$")
UNIT_PROPERTIES = ["User", "DynamicUser", "ProtectSystem", "ProtectHome", "PrivateTmp", "PrivateDevices",
                   "NoNewPrivileges", "CapabilityBoundingSet", "RestrictAddressFamilies",
                   "SystemCallFilter", "ReadWritePaths", "IPAddressDeny"]


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
            processes.append({"name": name, "pid": int(pid), "exe": exe, "package": store_name(exe), "unit": unit,
                              "unit_settings": unit_settings(unit)})
        sockets.append({"proto": proto, "local": local, "processes": processes})
    return sockets


def unit_settings(unit: str, cache: dict[str, dict[str, str]] = {}) -> dict[str, str]:
    """The unit's user and hardening, as systemd reports them."""
    if not unit.endswith(".service"):
        return {}
    if unit not in cache:
        result = subprocess.run(["systemctl", "show", unit, "--property=" + ",".join(UNIT_PROPERTIES)],
                                capture_output=True, text=True)
        cache[unit] = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
    return cache[unit]


def packages(out: Path) -> Path:
    """A vulnix packages.json from the system closure's store path names. tsugumi has no .drv
    files (systems are built on saya and copied), so vulnix can't read derivations; this loses
    the patch lists, so expect more false positives."""
    closure = subprocess.run(["nix-store", "--query", "--requisites", "/run/current-system"],
                             capture_output=True, text=True, check=True).stdout.split()
    found = {}
    for path in closure:
        name = store_name(path)
        if name is None or name.endswith((".drv", ".patch", ".tar.gz", ".tar.xz", ".zip")):
            continue
        name = OUTPUTS.sub("", name)
        if VERSIONED.match(name):
            found[name] = {"name": name, "patches": []}
    target = out / ".packages.json"
    target.write_text(json.dumps(found))
    return target


def vulnix(out: Path) -> dict[str, object]:
    cache = os.environ["VULNIX_CACHE"]
    argv = ["vulnix", "--json", "--cache-dir", cache, "--from-file", str(packages(out))]
    if os.environ.get("VULNIX_WHITELIST"):
        argv += ["--whitelist", os.environ["VULNIX_WHITELIST"]]
    result = subprocess.run(argv, capture_output=True, text=True, timeout=3600)
    # vulnix exits 2 when it found something, 1 on errors (e.g. no network for the NVD feed).
    try:
        findings = json.loads(result.stdout) if result.stdout.strip() else []
    except json.JSONDecodeError:
        findings = []
    if result.returncode not in (0, 2) or not isinstance(findings, list):
        raise RuntimeError(f"vulnix exited {result.returncode}: {result.stderr.strip()[-1500:]}")
    return {"exit": result.returncode, "findings": findings, "stderr": result.stderr[-4000:],
            "note": "matched by store path name and version only; patches are not considered"}


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
    for name, collect in (("listening.json", listening), ("mods.json", mods), ("vulnix.json", lambda: vulnix(out))):
        try:
            write(out, name, collect())
        except Exception as error:  # one failed collector shouldn't stop the others
            problems.append(f"{name}: {error}")
            write(out, name, {"error": str(error)})
    try:
        data = json.loads((out / "vulnix.json").read_text())
        found = f"{len(data['findings'])} vulnix findings" if "findings" in data else "vulnix FAILED"
    except (OSError, ValueError, KeyError, TypeError):
        found = "vulnix FAILED"
    note = f"Inventory in {out}: system, exposed, listening, mods, vulnix ({found})."
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
