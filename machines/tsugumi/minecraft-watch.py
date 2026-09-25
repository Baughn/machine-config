"""Snapshot watchdog: checks that Minecraft worlds keep being snapshotted,
replicated and served, and posts state changes to a Discord webhook.

See docs/agent-channel-design.md, "Snapshot watchdog". Checks are named
functions returning {key: Result}; a key fires after SETTINGS["confirm"]
consecutive failing runs and resolves on the first passing one.
"""

from collections import namedtuple
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.request

ROOT = "rpool/minecraft"
BACKUP_PREFIX = "stash/zrepl/rpool/"
PREFIX = "zrepl_"
STATE = Path("/var/lib/minecraft-watch")
LEASES = Path("/run/minecraft-save-hook")
STORAGE = Path("/var/lib/minecraft-storage")
ENV = {"PATH": "/var/empty", "LANG": "C.UTF-8"}
FILTERS = '@filters@'
AUTOSTART = '@autostart@'
OWNER = '@owner@'
WEBHOOK = Path('@webhook@')
MENTION = f"<@{OWNER}> " if OWNER else ""
THRESHOLDS = '@settings@'
SETTINGS = {}  # filled from THRESHOLDS by main()
USER_AGENT = "DiscordBot (https://github.com/baughn/machine-config, 1)"
LIMIT = 1900

Result = namedtuple("Result", "ok detail")


def run(*argv):
    return subprocess.run(argv, check=True, env=ENV, text=True,
                          stdout=subprocess.PIPE).stdout.strip()


def zfs(*args):
    return run("@zfs@", *args)


def exists(dataset):
    return subprocess.run(["@zfs@", "list", "-H", "-o", "name", dataset], env=ENV,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0


def worlds():
    output = zfs("list", "-H", "-o", "name", "-t", "filesystem", "-d", "1", ROOT)
    return [name for name in output.splitlines() if name != ROOT]


def newest(dataset):
    """Creation time of the newest zrepl snapshot, or None."""
    output = zfs("list", "-Hp", "-t", "snapshot", "-d", "1", "-o", "name,creation", dataset)
    times = [int(creation) for name, creation in
             (line.split("\t") for line in output.splitlines())
             if name.partition("@")[2].startswith(PREFIX)]
    return max(times, default=None)


def replicated(dataset):
    matches = [(len(pattern.rstrip("<")), not pattern.endswith("<"), enabled)
               for pattern, enabled in json.loads(FILTERS).items()
               if dataset == pattern or (pattern.endswith("<") and
                   (dataset == pattern[:-1] or dataset.startswith(pattern[:-1] + "/")))]
    return max(matches, default=(0, False, False))[2]


def unit(name, *props):
    output = run("@systemctl@", "show", *(f"--property={p}" for p in props), name)
    return dict(line.split("=", 1) for line in output.splitlines())


def uptime():
    return float(Path("/proc/uptime").read_text().split()[0])


def age(seconds):
    return f"{seconds // 3600} h {seconds % 3600 // 60} min" if seconds >= 3600 else f"{seconds // 60} min"


def check_snapshots(now, state):
    if uptime() < SETTINGS["bootGrace"]:
        return None
    results = {}
    for dataset in worlds():
        created = newest(dataset)
        world = dataset.rsplit("/", 1)[1]
        if created is None:
            results[world] = Result(False, f"{dataset} has no {PREFIX} snapshot")
        elif now - created > SETTINGS["snapshotAge"]:
            results[world] = Result(False, f"newest snapshot of {dataset} is {age(now - created)} old")
        else:
            results[world] = Result(True, "")
    return results


def check_replication(now, state):
    if uptime() < SETTINGS["bootGrace"]:
        return None
    results = {}
    for dataset in worlds():
        if not replicated(dataset):
            continue
        world = dataset.rsplit("/", 1)[1]
        backup = BACKUP_PREFIX + dataset
        created = newest(backup) if exists(backup) else None
        if created is None:
            results[world] = Result(False, f"{backup} has no {PREFIX} snapshot")
        elif now - created > SETTINGS["replicaAge"]:
            results[world] = Result(False, f"newest replica of {dataset} is {age(now - created)} old")
        else:
            results[world] = Result(True, "")
    return results


def check_leases(now, state):
    results = {}
    for directory in sorted(LEASES.iterdir()) if LEASES.is_dir() else []:
        pending = directory / "pending.json"
        try:
            held = now - int(pending.stat().st_mtime)
        except FileNotFoundError:
            results[directory.name] = Result(True, "")
            continue
        results[directory.name] = Result(
            held <= SETTINGS["leaseAge"],
            f"saving on {directory.name} has been off for {age(held)} (snapshot hook lease)")
    recovery = unit("minecraft-save-recovery.service", "ActiveState")["ActiveState"]
    results["recovery"] = Result(recovery != "failed", "minecraft-save-recovery.service has failed")
    return results


def check_zrepl(now, state):
    rollback = (STORAGE / "rollback.json").exists()
    active = unit("zrepl.service", "ActiveState")["ActiveState"] == "active"
    return {
        "rollback": Result(not rollback, "a rollback is in progress or was interrupted "
                           f"({STORAGE}/rollback.json); zrepl stays stopped until it completes"),
        "restart": Result(not (STORAGE / "complete.json").exists(),
                          "a rollback finished but restarting zrepl is still pending; "
                          "repeat the rollback command"),
        # While a rollback is pending, "rollback" already says why zrepl is down.
        "active": Result(active or rollback, "zrepl.service is not active"),
    }


def check_worlds(now, state):
    history = state.setdefault("restarts", {})
    results = {}
    for world in json.loads(AUTOSTART):
        props = unit(f"minecraft@{world}.service", "ActiveState", "SubState", "NRestarts")
        count = int(props.get("NRestarts") or 0)
        # NRestarts resets when the unit is started by hand; restart the window.
        samples = [s for s in history.get(world, [])
                   if now - s[0] <= SETTINGS["loopWindow"] and s[1] <= count]
        samples.append([now, count])
        history[world] = samples
        restarts = count - samples[0][1]
        if props["ActiveState"] != "active":
            results[world] = Result(False, f"minecraft@{world} is {props['ActiveState']} ({props['SubState']})")
        elif restarts >= SETTINGS["loopRestarts"]:
            results[world] = Result(False, f"minecraft@{world} restarted {restarts} times "
                                    f"in the last {age(SETTINGS['loopWindow'])}")
        else:
            results[world] = Result(True, "")
    for world in list(history):
        if world not in json.loads(AUTOSTART):
            del history[world]
    return results


CHECKS = [
    ("snapshot", check_snapshots),
    ("replica", check_replication),
    ("lease", check_leases),
    ("zrepl", check_zrepl),
    ("world", check_worlds),
]


def evaluate(now, state):
    """Run every check. Returns {key: Result} and the set of skipped check names."""
    results, skipped = {}, set()
    for name, check in CHECKS:
        try:
            found = check(now, state)
        except Exception as error:
            skipped.add(name)
            results[f"error:{name}"] = Result(False, f"check {name} failed: {error}")
            continue
        results[f"error:{name}"] = Result(True, "")
        if found is None:
            skipped.add(name)
            continue
        results.update({f"{name}:{key}": result for key, result in found.items()})
    return results, skipped


def transition(keys, results, skipped):
    """Advance per-key state. Returns (new keys, firing [(key, detail)], resolved [key])."""
    updated, firing, resolved = {}, [], []
    for key, entry in keys.items():
        if key not in results and key.split(":")[0] in skipped:
            updated[key] = entry
        elif key not in results and entry.get("fired"):
            # The subject disappeared (world deleted, lease released).
            resolved.append(key)
    for key, result in results.items():
        entry = dict(keys.get(key, {}))
        if result.ok:
            if entry.get("fired"):
                resolved.append(key)
            continue
        entry["failing"] = entry.get("failing", 0) + 1
        entry["detail"] = result.detail
        if not entry.get("fired") and entry["failing"] >= SETTINGS["confirm"]:
            entry["fired"] = True
            firing.append((key, result.detail))
        updated[key] = entry
    return updated, firing, resolved


def message(firing, resolved, keys):
    lines = [f"🔴 `{key}` {detail}" for key, detail in firing]
    lines += [f"🟢 resolved: `{key}` ({keys[key].get('detail', '')})" for key in resolved]
    text = (MENTION if firing else "") + "minecraft-watch\n"
    for index, line in enumerate(lines):
        if len(text) + len(line) + 40 > LIMIT:
            text += f"… and {len(lines) - index} more; run `minecraft-watch status` on tsugumi\n"
            break
        text += line + "\n"
    return text


def post(content):
    url = WEBHOOK.read_text().strip()
    body = json.dumps({"content": content, "username": "minecraft-watch",
                       "allowed_mentions": {"users": [OWNER] if OWNER else []}}).encode()
    request = urllib.request.Request(url, data=body, method="POST", headers={
        "Content-Type": "application/json", "User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        response.read()


def load():
    try:
        return json.loads((STATE / "state.json").read_text())
    except FileNotFoundError:
        return {}


def save(state):
    temporary = STATE / "state.json.tmp"
    temporary.write_text(json.dumps(state))
    os.replace(temporary, STATE / "state.json")


def watch(now=None):
    now = int(time.time()) if now is None else now
    state = load()
    old = state.get("keys", {})
    results, skipped = evaluate(now, state)
    keys, firing, resolved = transition(old, results, skipped)
    if firing or resolved:
        try:
            post(message(firing, resolved, {**old, **keys}))
        except Exception as error:
            # Keep the old fired flags, so the next run posts again.
            print(f"minecraft-watch: posting failed: {error}", file=sys.stderr)
            for key, _ in firing:
                keys[key]["fired"] = False
            for key in resolved:
                keys[key] = old[key]
            state["keys"] = keys
            save(state)
            return 1
    state["keys"] = keys
    save(state)
    return 0


def status():
    state = load()
    results, skipped = evaluate(int(time.time()), json.loads(json.dumps(state)))
    for key, result in sorted(results.items()):
        fired = state.get("keys", {}).get(key, {}).get("fired")
        mark = "ok  " if result.ok else ("FIRE" if fired else "fail")
        print(f"{mark} {key}" + ("" if result.ok else f": {result.detail}"))
    for name in sorted(skipped):
        print(f"skip {name}")
    return 0 if all(r.ok for r in results.values()) else 1


def main(argv):
    SETTINGS.update(json.loads(THRESHOLDS))
    match argv:
        case []:
            return watch()
        case ["status"]:
            return status()
        case ["test"]:
            post(MENTION + "minecraft-watch test post; nothing is wrong.")
            return 0
        case _:
            print("Usage: minecraft-watch [status|test]", file=sys.stderr)
            return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
