"""Inventory for the security watch (agent-sec.nix): what tsugumi exposes, what runs
behind it, how it behaves (connections, processes, load), and vulnix's view of the system closure. Runs as root before the agent's turn;
the agent only reads the JSON it writes, and nothing the agent wrote is run here.

Usage: agent-sec-inventory OUT_DIR EXPOSED_JSON TRIGGER_DIR AGENT_USER
Environment: VULNIX_WHITELIST, VULNIX_CACHE, MC_ROOT, MC_USER, MC_WORLDS (space-separated),
NIXPKGS_PATH, NIXPKGS_VERSION, MACHINE_CONFIG_REV, FLAKE_INPUTS and BUILT (JSON).
"""

from __future__ import annotations

import hashlib
import hmac
import ipaddress
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


# --- Behavioural data (Baughn, msg 1562834074505056381; Garibaldi's list, msg 1562834245993373738;
# plan: saya's notes/plan-sec-behaviour.md). Garibaldi posts in a public channel, so inbound peers
# (players) are aggregated to prefixes, and secret-looking argv values are redacted.

SECRET_ARG = re.compile(r"(?i)((?:pass|passwd|password|token|secret|key|auth|credential)[\w.-]*[=:])\S+")
SECRET_FLAG = re.compile(r"(?i)^-{1,2}(?:p|u|pass|passwd|password|token|secret|api[-_]?key|key|auth|user)$")
USERINFO = re.compile(r"(?<=://)[^/@\s:]+:[^/@\s]+(?=@)")
INTERPRETERS = re.compile(r"^(?:sh|bash|dash|zsh|fish|ash|busybox|python[\d.]*|perl[\d.]*|ruby|node|nc|ncat|"
                          r"netcat|socat|curl|wget|telnet|php)$")
SUSPECT_DIRS = ("/tmp/", "/var/tmp/", "/dev/shm/", "/run/user/")
SEEN_DAYS = 30  # an outbound destination unseen this long counts as new again
HISTORY = 42  # load samples kept: 7 days at one every 4 h


def redact(argv: list[str], limit: int) -> str:
    """Blank secret-looking values per argument (key=value, `--password VALUE`, URL user:pass)."""
    clean = []
    for i, arg in enumerate(argv):
        if i > 0 and SECRET_FLAG.match(argv[i - 1]):
            clean.append("<redacted>")
            continue
        clean.append(USERINFO.sub("<redacted>", SECRET_ARG.sub(r"\1<redacted>", arg)))
    text = shlex.join(clean)
    return text if len(text) <= limit else text[:limit] + "…"


def unit_of(cgroup: str) -> str:
    """The innermost service or scope in a /proc/PID/cgroup text (cgroup v2)."""
    path = cgroup.strip().splitlines()[-1].split("::", 1)[-1] if cgroup.strip() else ""
    parts = [part for part in path.split("/") if part]
    for part in reversed(parts):
        if part.endswith((".service", ".scope")):
            return part
    return parts[-1] if parts else "-"


def read_state(out: Path, name: str) -> dict[str, object]:
    try:
        data = json.loads((out / name).read_text())
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def split_address(address: str) -> tuple[str, str]:
    """ss's host:port, [v6]:port or host%iface:port -> (host, port); ::ffff:a.b.c.d -> a.b.c.d."""
    host, _, port = address.rpartition(":")
    host = host.strip("[]").split("%", 1)[0]
    try:
        mapped = ipaddress.ip_address(host)
        if mapped.version == 6 and mapped.ipv4_mapped is not None:
            host = str(mapped.ipv4_mapped)
    except ValueError:
        pass
    return host, port


def prefix(host: str) -> str:
    """A peer's network, for counting players without listing their addresses."""
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return host
    if address.is_private or address.is_loopback or address.is_link_local:
        return host
    bits = 24 if address.version == 4 else 48
    return str(ipaddress.ip_network(f"{host}/{bits}", strict=False))


def parse_ss(output: str) -> list[dict[str, object]]:
    """`ss -Htuanp` lines -> proto, state, local, peer and the (name, pid) owners."""
    rows = []
    for line in output.splitlines():
        fields = line.split()
        if len(fields) < 6:
            continue
        rows.append({"proto": fields[0], "state": fields[1], "local": fields[4], "peer": fields[5],
                     "owners": [(name, int(pid)) for name, pid in re.findall(r'\("([^"]*)",pid=(\d+)', line)]})
    return rows


def owner(pid: int, cache: dict[int, dict[str, str]]) -> dict[str, str]:
    if pid not in cache:
        info = {"exe": "", "unit": "", "user": ""}
        try:
            info["exe"] = os.readlink(f"/proc/{pid}/exe")
            info["unit"] = unit_of(Path(f"/proc/{pid}/cgroup").read_text())
            info["user"] = user_name(Path(f"/proc/{pid}").stat().st_uid)
        except OSError:
            pass
        cache[pid] = info
    return cache[pid]


def user_name(uid: int) -> str:
    try:
        return pwd.getpwuid(uid).pw_name
    except KeyError:
        return str(uid)


def classify_sockets(rows: list[dict[str, object]], cache: dict[int, dict[str, str]]) -> dict[str, object]:
    """Established sockets: outbound peers in full, inbound ones counted per local port by prefix.
    A socket is inbound when its local port is one we listen on."""
    listeners = {(row["proto"], split_address(str(row["local"]))[1]) for row in rows
                 if row["state"] in ("LISTEN", "UNCONN")}
    outbound: dict[tuple[str, ...], int] = {}
    inbound: dict[tuple[str, str], dict[str, int]] = {}
    loopback = 0
    for row in rows:
        if row["state"] != "ESTAB":
            continue
        proto = str(row["proto"])
        _, local_port = split_address(str(row["local"]))
        peer, peer_port = split_address(str(row["peer"]))
        try:
            if ipaddress.ip_address(peer).is_loopback:
                loopback += 1
                continue
        except ValueError:
            pass
        owners = [owner(pid, cache) for _, pid in row["owners"]] or [{"exe": "", "unit": "", "user": ""}]  # type: ignore[union-attr]
        if (proto, local_port) in listeners:
            counts = inbound.setdefault((proto, local_port), {})
            counts[prefix(peer)] = counts.get(prefix(peer), 0) + 1
        else:
            first = owners[0]
            key = (proto, peer, peer_port, first["unit"], first["exe"], first["user"])
            outbound[key] = outbound.get(key, 0) + 1
    return {
        "outbound": [{"proto": k[0], "peer": k[1], "port": k[2], "unit": k[3], "exe": k[4], "user": k[5], "count": n}
                     for k, n in sorted(outbound.items())],
        "inbound": [{"proto": k[0], "local_port": k[1], "connections": sum(c.values()), "sources": len(c),
                     "top_prefixes": dict(sorted(c.items(), key=lambda item: -item[1])[:10])}
                    for k, c in sorted(inbound.items())],
        "loopback_connections": loopback,
    }


CONNTRACK = re.compile(r"^(\S+)\s+\d+\s+(?:\d+\s+)?(?:([A-Z_]+)\s+)?.*?src=(\S+) dst=(\S+)(?: sport=(\d+) dport=(\d+))?")


def parse_conntrack(output: str, local: set[str]) -> dict[str, object]:
    """conntrack -L lines, summarised: outbound destinations in full, inbound per port by prefix."""
    outbound: dict[tuple[str, str, str], int] = {}
    inbound: dict[tuple[str, str], dict[str, int]] = {}
    forwarded = 0
    for line in output.splitlines():
        match = CONNTRACK.match(line.strip())
        if match is None:
            continue
        proto, _, src, dst, _, dport = match.groups()
        dport = dport or "-"
        if src in local and dst in local:
            continue
        if src in local:
            key = (proto, dst, dport)
            outbound[key] = outbound.get(key, 0) + 1
        elif dst in local:
            counts = inbound.setdefault((proto, dport), {})
            counts[prefix(src)] = counts.get(prefix(src), 0) + 1
        else:
            forwarded += 1
    return {
        "outbound": [{"proto": k[0], "peer": k[1], "port": k[2], "count": n}
                     for k, n in sorted(outbound.items(), key=lambda item: -item[1])[:200]],
        "outbound_destinations": len(outbound),
        "inbound": [{"proto": k[0], "port": k[1], "flows": sum(c.values()), "sources": len(c),
                     "top_prefixes": dict(sorted(c.items(), key=lambda item: -item[1])[:10])}
                    for k, c in sorted(inbound.items(), key=lambda item: -sum(item[1].values()))[:50]],
        "forwarded": forwarded,
    }


def local_addresses() -> set[str]:
    data = json.loads(subprocess.run(["ip", "-j", "addr"], capture_output=True, text=True, check=True).stdout)
    return {info["local"] for interface in data for info in interface.get("addr_info", []) if "local" in info}


def mark_new(out: Path, name: str, keys: list[str], now: float) -> list[str]:
    """Keys not seen in the last SEEN_DAYS; remembers them all. Only a finished collector calls this,
    so a failed run doesn't forget anything."""
    seen = {k: float(v) for k, v in read_state(out, name).items() if isinstance(v, (int, float))}
    fresh = sorted(k for k in set(keys) if k not in seen or now - seen[k] > SEEN_DAYS * 86400)
    first_run = not seen
    seen.update({k: now for k in keys})
    write(out, name, {k: v for k, v in seen.items() if now - v <= SEEN_DAYS * 86400})
    return [] if first_run else fresh


def flows(out: Path) -> dict[str, object]:
    now = time.time()
    rows = parse_ss(subprocess.run(["ss", "-Htuanp"], capture_output=True, text=True, check=True).stdout)
    data: dict[str, object] = {"generated": int(now), "sockets": classify_sockets(rows, {})}
    local = local_addresses()
    tracked, errors = [], []
    for family in ("ipv4", "ipv6"):
        result = subprocess.run(["conntrack", "-L", "-f", family], capture_output=True, text=True)
        tracked.append(result.stdout)
        if result.returncode != 0:
            errors.append(f"{family}: exit {result.returncode}: {result.stderr.strip()[-300:]}")
    data["conntrack"] = parse_conntrack("\n".join(tracked), local)
    if errors:
        data["conntrack"]["error"] = errors  # type: ignore[index]
    sockets = data["sockets"]["outbound"]  # type: ignore[index]
    keys = [f"{s['proto']} {s['peer']}:{s['port']} {s['unit']}" for s in sockets]  # type: ignore[index]
    keys += [f"{c['proto']} {c['peer']}:{c['port']} (conntrack)" for c in data["conntrack"]["outbound"]]  # type: ignore[index]
    data["new_outbound"] = mark_new(out, ".flows-seen.json", keys, now)
    data["note"] = ("A snapshot: a connection that opens and closes between runs isn't here, so an empty "
                    "new_outbound is not proof of absence. new_outbound: destinations not seen in the last "
                    "30 days (empty on the first run). Inbound peers are counted by /24 or /48, not listed.")
    return data


def proc_stat(text: str) -> tuple[int, int, int]:
    """/proc/PID/stat -> (ppid, CPU ticks, start ticks); the name may contain spaces and parens."""
    fields = text[text.rindex(")") + 2:].split()
    return int(fields[1]), int(fields[11]) + int(fields[12]), int(fields[19])


def process_flags(name: str, exe: str, cwd: str, uid: int, unit: str) -> list[str]:
    flags = []
    if exe.endswith(" (deleted)"):
        flags.append("exe deleted")
    elif not exe.startswith("/nix/store/"):
        flags.append("exe outside /nix/store")
    if exe.startswith(SUSPECT_DIRS) or cwd.startswith(SUSPECT_DIRS) or cwd.rstrip("/") in ("/tmp", "/var/tmp", "/dev/shm"):
        flags.append("runs from a temp dir")
    if INTERPRETERS.match(name) and 0 < uid < 1000:
        flags.append("shell or interpreter under a system user")
    if not unit.endswith((".service", ".scope")) and unit != "init.scope":
        flags.append("not in a unit")
    return flags


def processes(out: Path) -> dict[str, object]:
    ticks = os.sysconf("SC_CLK_TCK")
    boot = time.time() - float(Path("/proc/uptime").read_text().split()[0])
    flagged, by_unit, kernel, skipped = [], {}, 0, 0
    exes = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            try:
                exe = os.readlink(proc / "exe")
            except FileNotFoundError:  # kernel threads have no executable
                kernel += 1
                continue
            cmdline = (proc / "cmdline").read_bytes()
            argv = [arg.decode(errors="replace") for arg in cmdline.split(b"\0") if arg]
            try:
                cwd = os.readlink(proc / "cwd")
            except OSError:
                cwd = ""
            uid = proc.stat().st_uid
            unit = unit_of((proc / "cgroup").read_text(errors="replace"))
            ppid, cpu, start = proc_stat((proc / "stat").read_text(errors="replace"))
            rss = next((int(line.split()[1]) for line in (proc / "status").read_text(errors="replace").splitlines()
                        if line.startswith("VmRSS:")), 0)
        except (OSError, ValueError, IndexError):
            skipped += 1  # mostly processes that exited meanwhile
            continue
        name = Path(exe.removesuffix(" (deleted)")).name
        user = user_name(uid)
        entry = by_unit.setdefault(unit, {"users": set(), "exes": {}, "processes": 0, "rss_kb": 0})
        entry["users"].add(user)
        entry["exes"][exe] = entry["exes"].get(exe, 0) + 1
        entry["processes"] += 1
        entry["rss_kb"] += rss
        exes.append(f"{unit} {exe}")
        flags = process_flags(name, exe, cwd, uid, unit)
        if flags:
            flagged.append({"pid": int(proc.name), "ppid": ppid, "user": user, "unit": unit, "exe": exe, "cwd": cwd,
                            "argv": redact(argv, 1024), "started": int(boot + start / ticks),
                            "cpu_seconds": round(cpu / ticks, 1), "rss_kb": rss, "flags": flags})
    units = {unit: {"users": sorted(e["users"]), "processes": e["processes"], "rss_kb": e["rss_kb"],
                    "exes": dict(sorted(e["exes"].items()))} for unit, e in sorted(by_unit.items())}
    return {"generated": int(time.time()), "kernel_threads": kernel, "unreadable": skipped,
            "flagged": flagged, "units": units,
            "new_exes": mark_new(out, ".exes-seen.json", exes, time.time()),
            "note": "units: every process grouped by unit and executable. flagged: the full (redacted) "
                    "command line of each process with a flag. new_exes: unit+executable pairs not seen in "
                    "30 days (store paths change on every deploy)."}


def cgroups(root: Path) -> dict[str, tuple[int, int]]:
    """Unit cgroup -> (CPU microseconds so far, memory bytes now), for services and user slices."""
    found = {}
    # Template instances (minecraft-lab@x.service) sit one level down, in system-<name>.slice.
    for pattern in ("system.slice/*.service", "system.slice/*.slice/*.service", "user.slice/user-*.slice",
                    "machine.slice/*"):
        for group in root.glob(pattern):
            try:
                usage = next(int(line.split()[1]) for line in (group / "cpu.stat").read_text().splitlines()
                             if line.startswith("usage_usec "))
                memory = int((group / "memory.current").read_text())
            except (OSError, ValueError, StopIteration):
                continue
            found[group.name] = (usage, memory)
    return found


def load_report(state: dict[str, object], now: float, current: dict[str, tuple[int, int]]) -> tuple[dict[str, object], dict[str, object]]:
    """CPU cores per unit since the last run, against that unit's own history. Returns (report, new state)."""
    last_time = float(state.get("time") or 0)  # type: ignore[arg-type]
    last = state.get("usage") or {}
    history: dict[str, list[float]] = {k: list(v) for k, v in (state.get("history") or {}).items()}  # type: ignore[union-attr]
    interval = now - last_time
    if "time" in state and interval < 1800:
        # A manual rerun minutes later: measure from the earlier run next time instead.
        return ({"interval_seconds": round(interval), "units": {u: {"memory_mb": round(m / 2**20)}
                                                                for u, (_, m) in sorted(current.items())},
                 "unusual": [], "skipped": "under 30 min since the previous run; no CPU figures"}, state)
    units = {}
    for unit, (usage, memory) in sorted(current.items()):
        entry: dict[str, object] = {"memory_mb": round(memory / 2**20)}
        previous = last.get(unit) if isinstance(last, dict) else None
        if previous is not None and 0 < interval < 86400 and usage >= previous:
            cores = round((usage - previous) / 1e6 / interval, 3)
            entry["cpu_cores"] = cores
            past = history.get(unit, [])
            if len(past) >= 6:
                top = max(past)
                entry["baseline"] = {"mean": round(sum(past) / len(past), 3), "max": top, "samples": len(past)}
                entry["unusual"] = cores > max(2 * top, top + 0.5) and cores >= 0.25
            else:
                entry["baseline"] = f"none yet ({len(past)} samples)"
            if not entry.get("unusual"):  # a lasting spike shouldn't become its own baseline
                history[unit] = (past + [cores])[-HISTORY:]
        units[unit] = entry
    report = {"interval_seconds": round(interval) if "time" in state else None, "units": units,
              "unusual": sorted(u for u, e in units.items() if e.get("unusual"))}
    return report, {"time": now, "usage": {u: v[0] for u, v in current.items()},
                    "history": {u: h for u, h in history.items() if u in current}}


def gpus() -> list[dict[str, object]]:
    found = []
    for card in sorted(Path("/sys/class/drm").glob("card[0-9]*")):
        if "-" in card.name:
            continue
        busy = card / "device" / "gpu_busy_percent"
        entry: dict[str, object] = {"card": card.name, "driver": os.path.basename(os.path.realpath(card / "device" / "driver"))}
        try:
            entry["busy_percent"] = int(busy.read_text())
        except (OSError, ValueError):
            pass
        found.append(entry)
    return found


def load(out: Path) -> dict[str, object]:
    now = time.time()
    report, state = load_report(read_state(out, ".load-state.json"), now, cgroups(Path("/sys/fs/cgroup")))
    write(out, ".load-state.json", state)
    report["generated"] = int(now)
    report["loadavg"] = Path("/proc/loadavg").read_text().split()[:3]
    report["gpus"] = gpus() or "none detected"
    report["note"] = ("cpu_cores: average cores used since the previous run. unusual: above twice the unit's "
                      "7-day max and at least half a core more; needs 6 samples first. Unusual samples are "
                      "kept out of the baseline, so a lasting spike stays unusual.")
    return report


# --- Slice B: logins and persistence (same plan). Accepted ssh sources are Baughn's own addresses,
# so they appear as keyed hashes; the key is readable by root only, not by the agent.

SSH_ACCEPTED = re.compile(r"^Accepted (\S+) for (\S+) from (\S+) port \d+")
SSH_FAILED = re.compile(r"^Failed (\S+) for (?:invalid user )?(\S+) from (\S+) port \d+")
SSH_INVALID = re.compile(r"^Invalid user (\S*) from (\S+)")
SUDO = re.compile(r"^\s*(\S+) : .*?USER=(\S+) ; COMMAND=(.*)$")
LOGIND = re.compile(r"^New session \S+ of user (\S+)\.")


def source_key(out: Path) -> bytes:
    path = out / ".source-key"
    try:
        return path.read_bytes()
    except FileNotFoundError:
        key = os.urandom(32)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "wb") as file:
            file.write(key)
        return key


def hashed(key: bytes, host: str) -> str:
    return "src-" + hmac.new(key, host.encode(), hashlib.sha256).hexdigest()[:12]


def parse_auth(entries: list[dict[str, object]], key: bytes) -> dict[str, object]:
    """Journal entries (sshd, sudo, su, logind) -> who logged in, failures, sudo use."""
    accepted: dict[tuple[str, str, str], int] = {}
    failed_users: dict[str, int] = {}
    failed_sources: dict[str, int] = {}
    sudo, su, sessions = [], [], {}
    for entry in entries:
        message = str(entry.get("MESSAGE") or "")
        ident = str(entry.get("SYSLOG_IDENTIFIER") or "")
        if ident.startswith("sshd"):
            match = SSH_ACCEPTED.match(message)
            if match:
                k = (match.group(2), match.group(1), hashed(key, match.group(3)))
                accepted[k] = accepted.get(k, 0) + 1
                continue
            match = SSH_FAILED.match(message) or SSH_INVALID.match(message)
            if match:
                user, host = match.groups()[-2:]
                failed_users[user] = failed_users.get(user, 0) + 1
                failed_sources[prefix(host)] = failed_sources.get(prefix(host), 0) + 1
        elif ident == "sudo":
            match = SUDO.match(message)
            if match:
                command = SECRET_ARG.sub(r"\1<redacted>", match.group(3))
                sudo.append({"user": match.group(1), "as": match.group(2), "command": command[:300],
                             "time": int(int(str(entry.get("__REALTIME_TIMESTAMP") or 0)) / 1e6)})
        elif ident == "su":
            su.append(message[:200])
        elif ident == "systemd-logind":
            match = LOGIND.match(message)
            if match:
                sessions[match.group(1)] = sessions.get(match.group(1), 0) + 1
    return {
        "ssh_accepted": [{"user": k[0], "method": k[1], "source": k[2], "count": n} for k, n in sorted(accepted.items())],
        "ssh_failed": {"total": sum(failed_users.values()),
                       "users": dict(sorted(failed_users.items(), key=lambda i: -i[1])[:20]),
                       "top_prefixes": dict(sorted(failed_sources.items(), key=lambda i: -i[1])[:20])},
        "sudo": sudo[-100:], "su": su[-50:], "new_sessions": sessions,
    }


def watched_files() -> list[Path]:
    """Account and key files whose changes matter."""
    files = [Path(p) for p in ("/etc/passwd", "/etc/group", "/etc/shadow", "/etc/sudoers", "/etc/ld.so.preload")]
    files += sorted(Path("/etc/ssh/authorized_keys.d").glob("*"))
    for account in pwd.getpwall():
        for name in ("authorized_keys", "authorized_keys2"):
            files.append(Path(account.pw_dir) / ".ssh" / name)
    return files


def file_hashes(paths: list[Path]) -> dict[str, str]:
    found = {}
    for path in paths:
        try:
            found[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()[:16]
        except (FileNotFoundError, NotADirectoryError):
            continue
        except OSError as error:
            found[str(path)] = f"unreadable: {error.strerror}"
    return found


def compare(previous: dict[str, object], current: dict[str, str]) -> dict[str, list[str]]:
    return {"added": sorted(current.keys() - previous.keys()), "removed": sorted(previous.keys() - current.keys()),
            "changed": sorted(k for k in current.keys() & previous.keys() if current[k] != previous[k])}


def auth(out: Path) -> dict[str, object]:
    state = read_state(out, ".auth-state.json")
    argv = ["journalctl", "-o", "json", "--no-pager"]
    argv += ["--after-cursor", str(state["cursor"])] if state.get("cursor") else ["--since", "-4h"]
    argv += ["SYSLOG_IDENTIFIER=sshd", "+", "SYSLOG_IDENTIFIER=sshd-session", "+", "SYSLOG_IDENTIFIER=sudo",
             "+", "SYSLOG_IDENTIFIER=su", "+", "SYSLOG_IDENTIFIER=systemd-logind"]
    result = subprocess.run(argv, capture_output=True, text=True)
    if result.returncode != 0 and state.get("cursor"):  # the cursor's journal file was rotated away
        argv[argv.index("--after-cursor"):argv.index("--after-cursor") + 2] = ["--since", "-4h"]
        result = subprocess.run(argv, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"journalctl exited {result.returncode}: {result.stderr.strip()[-300:]}")
    entries = []
    for line in result.stdout.splitlines():
        try:
            entries.append(json.loads(line))
        except ValueError:
            continue
    data = parse_auth(entries, source_key(out))
    data["new_ssh_sources"] = mark_new(out, ".ssh-sources-seen.json",
                                       [a["source"] for a in data["ssh_accepted"]], time.time())  # type: ignore[index]
    hashes = file_hashes(watched_files())
    data["files"] = hashes
    data["files_since_last_run"] = compare(state.get("files") or {}, hashes)  # type: ignore[arg-type]
    data["generated"] = int(time.time())
    data["note"] = ("Since the previous run. ssh sources are keyed hashes (same address, same hash); "
                    "new_ssh_sources: not seen in 30 days. Failed logins are counted by /24 or /48. "
                    "files: sha256 prefixes of account and key files; files_since_last_run: what changed.")
    cursor = next((e.get("__CURSOR") for e in reversed(entries) if e.get("__CURSOR")), state.get("cursor"))
    write(out, ".auth-state.json", {"cursor": cursor, "files": hashes})
    return data


def unit_sources(names: list[str]) -> list[dict[str, str]]:
    """systemctl show for many units at once -> Id, FragmentPath, DropInPaths, Transient."""
    found = []
    for start in range(0, len(names), 200):
        output = subprocess.run(["systemctl", "show", "--no-pager", "-p", "Id,FragmentPath,DropInPaths,Transient",
                                 "--", *names[start:start + 200]], capture_output=True, text=True).stdout
        for block in output.strip().split("\n\n"):
            found.append(dict(line.split("=", 1) for line in block.splitlines() if "=" in line))
    return found


def outside_store(path: str) -> bool:
    """Not from the NixOS config: neither in the store nor made by a generator (fstab, ZFS mounts)."""
    return bool(path) and not path.startswith("/run/systemd/generator") \
        and not os.path.realpath(path).startswith("/nix/store/")


def foreign_units(units: list[dict[str, str]]) -> dict[str, object]:
    """Units whose definition or drop-ins don't come from the store, i.e. not from the NixOS config."""
    foreign, transient, masked = [], [], []
    for unit in units:
        name = unit.get("Id", "")
        if unit.get("FragmentPath") and os.path.realpath(unit["FragmentPath"]) == "/dev/null":
            masked.append(name)
            continue
        if unit.get("Transient") == "yes":
            if not name.endswith(".scope"):  # session and app scopes are routine
                transient.append(name)
            continue
        reasons = []
        if outside_store(unit.get("FragmentPath", "")):
            reasons.append(f"defined in {unit['FragmentPath']}")
        for dropin in unit.get("DropInPaths", "").split():
            if outside_store(dropin):
                reasons.append(f"drop-in {dropin}")
        if reasons:
            foreign.append({"unit": name, "why": reasons})
    return {"foreign": foreign, "transient": sorted(transient), "masked": sorted(masked)}


def listing(roots: list[Path], limit: int = 200) -> list[dict[str, object]]:
    """Regular files under roots, with size and hash prefix."""
    found: list[dict[str, object]] = []
    for root in roots:
        if not root.exists():
            continue
        for path in sorted(root.rglob("*")) if root.is_dir() else [root]:
            if len(found) >= limit:
                return found
            try:
                if path.is_file():
                    found.append({"path": str(path), "bytes": path.stat().st_size,
                                  "sha256": hashlib.sha256(path.read_bytes()).hexdigest()[:16]})
            except OSError:
                continue
    return found


def setid_files() -> list[str]:
    """setuid/setgid files where none should be (the store and /run/wrappers are expected)."""
    result = subprocess.run(["find", "/etc", "/root", "/tmp", "/var/tmp", "/dev/shm", "/usr", "/bin", "/sbin",
                             "/opt", "/srv", "-xdev", "-type", "f", "(", "-perm", "-4000", "-o", "-perm", "-2000",
                             ")"], capture_output=True, text=True)
    return sorted(result.stdout.split())[:100]


def persistence(out: Path) -> dict[str, object]:
    listed = json.loads(subprocess.run(["systemctl", "list-units", "--all", "-o", "json", "--no-pager"],
                                       capture_output=True, text=True, check=True).stdout)
    names = sorted(u["unit"] for u in listed if u.get("load") != "not-found")
    data = foreign_units(unit_sources(names))
    timers = json.loads(subprocess.run(["systemctl", "list-timers", "--all", "-o", "json", "--no-pager"],
                                       capture_output=True, text=True, check=True).stdout)
    data["timers"] = sorted(f"{t.get('unit')} -> {t.get('activates')}" for t in timers)
    now = time.time()
    data["new_units"] = mark_new(out, ".units-seen.json", names, now)
    data["new_timers"] = mark_new(out, ".timers-seen.json", data["timers"], now)  # type: ignore[arg-type]
    homes = {Path(a.pw_dir) for a in pwd.getpwall() if a.pw_dir not in ("", "/", "/var/empty", "/nonexistent")}
    data["user_units"] = listing([home / ".config" / "systemd" / "user" for home in sorted(homes)])
    data["lingering"] = sorted(p.name for p in Path("/var/lib/systemd/linger").glob("*"))
    data["cron"] = listing([Path("/var/spool/cron"), Path("/etc/crontab"), Path("/etc/cron.d")])
    data["setid_outside_store"] = setid_files()
    data["generated"] = int(now)
    data["note"] = ("foreign: loaded units defined or overridden outside /nix/store (not from the NixOS config). "
                    "transient: non-scope units made at runtime (systemd-run). new_units/new_timers: not seen in "
                    "30 days, so expect some after a deploy.")
    return data


def write(out: Path, name: str, data: object) -> None:
    tmp = out / f".{name}.tmp"
    tmp.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
    tmp.chmod(0o640)
    tmp.rename(out / name)


def is_daily(out: Path, now: time.struct_time) -> bool:
    """The first run on or after 05:00 each day is the full daily review; the others (every four
    hours, Baughn msg 1562834074505056381) are change checks. Decided before the slow collectors,
    so a slow vulnix doesn't push the 05:30 run past the hour."""
    try:
        last = (out / ".last-daily").read_text().strip()
    except OSError:
        last = ""
    return now.tm_hour >= 5 and last != time.strftime("%Y-%m-%d", now)


def main() -> None:
    out, exposed, triggers, user = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4]
    out.mkdir(parents=True, exist_ok=True)
    now = time.localtime()
    daily = is_daily(out, now)
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
    collectors = [("listening.json", listening), ("mods.json", mods), ("jvms.json", jvms),
                  ("flows.json", lambda: flows(out)), ("processes.json", lambda: processes(out)),
                  ("load.json", lambda: load(out)), ("auth.json", lambda: auth(out)),
                  ("persistence.json", lambda: persistence(out))]
    # The NVD is slow: vulnix runs once a day, or when there is no result yet.
    if daily or not (out / "vulnix.json").exists():
        collectors.append(("vulnix.json", lambda: vulnix(out)))
    for name, collect in collectors:
        try:
            write(out, name, collect())
        except Exception as error:  # one failed collector shouldn't stop the others
            problems.append(f"{name}: {error}")
            write(out, name, {"error": str(error)})
    try:
        data = json.loads((out / "vulnix.json").read_text())
        found = f"{len(data['findings'])} vulnix findings" if "findings" in data else "vulnix FAILED"
        if not daily:
            found += ", from " + time.strftime("%Y-%m-%d %H:%M", time.localtime(data["generated"]))
        if data.get("diff", {}).get("baseline") is not None:
            diff = data["diff"]
            found += f"; {len(diff['new'])} new, {len(diff['gone'])} gone, {len(diff['changed'])} changed packages"
    except (OSError, ValueError, KeyError, TypeError):
        found = "vulnix FAILED"
    note = (f"Inventory in {out}: system, exposed, listening, mods, jvms, flows, processes, load, "
            f"auth, persistence, vulnix ({found}).")
    if problems:
        note += " Collector problems: " + "; ".join(problems)
    source = "daily" if daily else "check"
    account = pwd.getpwnam(user)
    stamp = time.strftime("%Y%m%dT%H%M%S")
    tmp = triggers / f"{source}-{stamp}.tmp"
    tmp.write_text(json.dumps({"source": source, "note": note[:1900]}))
    os.chown(tmp, account.pw_uid, account.pw_gid)
    tmp.rename(triggers / f"{source}-{stamp}.json")
    if daily:
        (out / ".last-daily").write_text(time.strftime("%Y-%m-%d", now) + "\n")


if __name__ == "__main__":
    main()
