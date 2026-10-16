"""Daily inventory for the security watch (agent-sec.nix): what tsugumi exposes, what runs
behind it, and vulnix's view of the system closure. Runs as root before the agent's turn;
the agent only reads the JSON it writes, and nothing the agent wrote is run here.

Usage: agent-sec-inventory OUT_DIR EXPOSED_JSON TRIGGER_DIR AGENT_USER
Environment: VULNIX_WHITELIST, VULNIX_CACHE, MC_ROOT, MC_USER, MC_WORLDS (space-separated),
NIXPKGS_PATH, NIXPKGS_VERSION, MACHINE_CONFIG_REV, FLAKE_INPUTS and BUILT (JSON).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import pwd
import re
import shlex
import shutil
import stat
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


def run_vulnix(argv: list[str]) -> tuple[subprocess.CompletedProcess[str], list[object]]:
    result = subprocess.run(argv, capture_output=True, text=True, timeout=3600)
    # vulnix exits 2 when it found something, 1 on errors (e.g. an NVD feed download timing out).
    try:
        findings = json.loads(result.stdout) if result.stdout.strip() else []
    except json.JSONDecodeError:
        findings = []
    if result.returncode not in (0, 2) or not isinstance(findings, list):
        raise RuntimeError(f"vulnix exited {result.returncode}: {result.stderr.strip()[-1500:]}")
    return result, findings


def by_package(findings: list[object]) -> dict[str, dict[str, set[str]]]:
    """pname -> its versions and CVEs, so a version bump isn't a vanished and a new package."""
    packages: dict[str, dict[str, set[str]]] = {}
    for item in findings:
        if not isinstance(item, dict):
            continue
        entry = packages.setdefault(str(item.get("pname") or item.get("name")), {"versions": set(), "cves": set()})
        entry["versions"].add(str(item.get("version")))
        entry["cves"].update(map(str, item.get("affected_by") or []))
    return packages


def vulnix_diff(previous: dict[str, object] | None, findings: list[object]) -> dict[str, object]:
    """What changed per package since the last successful run (Garibaldi's ask, msg 1560596485752754207)."""
    if previous is None:
        return {"baseline": None}
    old, new = by_package(previous.get("findings") or []), by_package(findings)  # type: ignore[arg-type]
    changed = []
    for pname in sorted(old.keys() & new.keys()):
        added, removed = new[pname]["cves"] - old[pname]["cves"], old[pname]["cves"] - new[pname]["cves"]
        if added or removed or old[pname]["versions"] != new[pname]["versions"]:
            changed.append({"pname": pname, "versions": sorted(old[pname]["versions"]),
                            "now": sorted(new[pname]["versions"]), "added": sorted(added), "removed": sorted(removed)})
    return {
        "baseline": previous.get("generated"),
        "new": [{"pname": p, "versions": sorted(new[p]["versions"]), "cves": sorted(new[p]["cves"])}
                for p in sorted(new.keys() - old.keys())],
        "gone": [{"pname": p, "versions": sorted(old[p]["versions"]), "cves": sorted(old[p]["cves"])}
                 for p in sorted(old.keys() - new.keys())],
        "changed": changed,
    }


def vulnix(out: Path, retry_delays: tuple[int, ...] = (120, 600)) -> dict[str, object]:
    cache = os.environ["VULNIX_CACHE"]
    argv = ["vulnix", "--json", "--cache-dir", cache, "--from-file", str(packages(out))]
    if os.environ.get("VULNIX_WHITELIST"):
        argv += ["--whitelist", os.environ["VULNIX_WHITELIST"]]
    # The NVD feeds time out now and then (16 Oct); two retries (Baughn, msg 1560596928151031839).
    errors = []
    for delay in (*retry_delays, None):
        try:
            result, findings = run_vulnix(argv)
            break
        except (RuntimeError, subprocess.TimeoutExpired) as error:
            errors.append(str(error)[-1500:])
            if delay is None:
                raise RuntimeError(f"vulnix failed {len(errors)} times; last: {errors[-1]}") from error
            time.sleep(delay)
    # The baseline is the last run that worked, so a failed day doesn't hide what changed.
    last_good = out / ".vulnix-last-good.json"
    previous = None
    # Before the first last-good copy, yesterday's vulnix.json serves, if it worked.
    for candidate in (last_good, out / "vulnix.json"):
        try:
            previous = json.loads(candidate.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(previous, dict) and "findings" in previous:
            break
        previous = None
    data = {"generated": int(time.time()), "exit": result.returncode, "findings": findings,
            "diff": vulnix_diff(previous, findings),
            "failed_attempts": errors, "stderr": result.stderr[-4000:],
            "note": "matched by store path name and version only; patches are not considered"}
    write(out, last_good.name, data)
    return data


def mods() -> dict[str, list[str]]:
    root = Path(os.environ.get("MC_ROOT", "/home/minecraft"))
    worlds = os.environ.get("MC_WORLDS", "").split()
    found = {}
    for world in worlds:
        directory = root / world / "mods"
        if directory.is_dir():
            found[world] = sorted(p.name for p in directory.iterdir() if p.suffix == ".jar")
    return found


def argfiles(argv: list[str], cwd: str, root: Path) -> dict[str, object]:
    """The contents of the JVM's @argfiles (NeoForge's user_jvm_args.txt and unix_args.txt),
    split roughly as java does (Garibaldi's ask, msg 1560596485752754207). We run as root and
    the files belong to the minecraft user, so only regular files under MC_ROOT are read: a
    symlink to /etc/shadow stays unread."""
    found: dict[str, object] = {}
    for arg in argv[1:]:
        if not arg.startswith("@") or arg.startswith("@@") or arg in found:
            continue
        try:
            fd = os.open(Path(cwd) / arg[1:], os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
        except OSError as error:
            found[arg] = {"error": str(error)}
            continue
        with os.fdopen(fd, "rb") as file:
            # Check what was opened, not the name, so swapping in a symlink after the check doesn't help.
            path = Path(os.readlink(f"/proc/self/fd/{fd}"))
            info = os.fstat(fd)
            if not path.is_relative_to(root):
                found[arg] = {"path": str(path), "error": f"outside {root}, not read"}
                continue
            if not stat.S_ISREG(info.st_mode) or info.st_size > 256 * 1024:
                found[arg] = {"path": str(path), "error": "not a regular file of at most 256 KiB"}
                continue
            text = file.read(256 * 1024).decode(errors="replace")
        try:
            found[arg] = {"path": str(path), "args": shlex.split(text, comments=True)}
        except ValueError as error:  # unbalanced quotes
            found[arg] = {"path": str(path), "error": str(error), "text": text}
    return found


def jvms() -> list[dict[str, object]]:
    """Every JVM running as the minecraft user, with its full command line (Baughn, msg
    1560250720479870977: for checking flags like jdk.serialFilter)."""
    root = Path(os.path.realpath(os.environ.get("MC_ROOT", "/home/minecraft")))
    uid = pwd.getpwnam(os.environ.get("MC_USER", "minecraft")).pw_uid
    found = []
    for proc in sorted(Path("/proc").iterdir(), key=lambda p: (len(p.name), p.name)):
        if not proc.name.isdigit():
            continue
        try:
            if proc.stat().st_uid != uid:
                continue
            argv = [arg.decode(errors="replace") for arg in (proc / "cmdline").read_bytes().split(b"\0")[:-1]]
            exe = os.readlink(proc / "exe")
            cwd = os.readlink(proc / "cwd")
            unit = (proc / "cgroup").read_text().strip().rsplit("/", 1)[-1]
        except OSError:
            continue
        if not argv or Path(exe).name != "java":
            continue
        found.append({"pid": int(proc.name), "exe": exe, "package": store_name(exe), "cwd": cwd,
                      "unit": unit, "argv": argv, "argfiles": argfiles(argv, cwd, root)})
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
        "machine_config_rev": os.environ.get("MACHINE_CONFIG_REV") or None,
        "flake_inputs": json.loads(os.environ.get("FLAKE_INPUTS") or "{}"),
        "built": json.loads(os.environ.get("BUILT") or "{}"),
    })
    shutil.copyfile(exposed, out / ".exposed.tmp")
    (out / ".exposed.tmp").chmod(0o640)
    (out / ".exposed.tmp").rename(out / "exposed.json")
    for name, collect in (("listening.json", listening), ("mods.json", mods), ("jvms.json", jvms),
                          ("vulnix.json", lambda: vulnix(out))):
        try:
            write(out, name, collect())
        except Exception as error:  # one failed collector shouldn't stop the others
            problems.append(f"{name}: {error}")
            write(out, name, {"error": str(error)})
    try:
        data = json.loads((out / "vulnix.json").read_text())
        found = f"{len(data['findings'])} vulnix findings" if "findings" in data else "vulnix FAILED"
        if data.get("diff", {}).get("baseline") is not None:
            diff = data["diff"]
            found += f"; {len(diff['new'])} new, {len(diff['gone'])} gone, {len(diff['changed'])} changed packages"
    except (OSError, ValueError, KeyError, TypeError):
        found = "vulnix FAILED"
    note = f"Inventory in {out}: system, exposed, listening, mods, jvms, vulnix ({found})."
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
