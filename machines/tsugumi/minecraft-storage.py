"""Narrow sudo interface for Minecraft snapshots and coordinated ZFS rewinds.

Only logical rpool/minecraft names cross the public interface. See
minecraft-storage.md for rollback recovery and the replication assumptions.
"""

from contextlib import contextmanager
import ctypes
import fcntl
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import time

ROOT = "rpool/minecraft"
BACKUP_PREFIX = "stash/zrepl/rpool/"
MOUNTPOINT = "/run/minecraft-snapshot"
STATE = Path("/var/lib/minecraft-storage")
ENV = {"PATH": "@path@", "LANG": "C.UTF-8"}
FILTERS = '@filters@'
MOUNT_HOLD = "minecraft-storage:mount"
ROLLBACK_HOLD = "minecraft-storage:rollback"
SENDER_HOLD = "zrepl_STEP_J_rpool"
RECEIVER_HOLD = "zrepl_last_received_J_backup-sink"
MANAGED_BOOKMARK = re.compile(r"zrepl_(?:CURSOR|CURSORTENTATIVE_?)_G_[0-9a-f]{16}_J_rpool\Z")
LOCK_FD = None
SNAPSHOT = re.compile(
    r"rpool/minecraft(?:/[A-Za-z0-9_][A-Za-z0-9_.:-]*)*"
    r"@[A-Za-z0-9_][A-Za-z0-9_.:-]*\Z"
)


def command(arguments):
    """Validate the public interface before running any subprocess."""
    match arguments:
        case [operation] if operation in ("status", "pools", "snapshots", "unmount"):
            return operation, None
        case [operation, snapshot] if operation in ("snapshot", "rollback", "mount"):
            if len(snapshot) > 255 or SNAPSHOT.fullmatch(snapshot) is None:
                raise ValueError("Expected a snapshot beneath rpool/minecraft")
            return operation, snapshot
        case _:
            raise ValueError(
                "Usage: minecraft-storage status|pools|snapshots|unmount "
                "or snapshot|rollback|mount DATASET@SNAPSHOT"
            )


def run(*argv, capture=False, check=True, **kwargs):
    kwargs["pass_fds"] = child_fds(kwargs.get("pass_fds", ()))
    return subprocess.run(argv, check=check, env=ENV, text=True,
                          stdout=subprocess.PIPE if capture else None, **kwargs)


def child_fds(extra=()):
    # An orphaned receive must finish before another invocation can recover it.
    return (*extra, *((LOCK_FD,) if LOCK_FD is not None else ()))


def zfs(*args):
    return run("@zfs@", *args, capture=True).stdout.strip()


def filesystems():
    return set(zfs("list", "-H", "-t", "filesystem", "-o", "name").splitlines())


def versions(dataset, recursive=False):
    """TXGs are comparable only within one pool; GUIDs identify shared history."""
    output = zfs("list", "-Hp", "-t", "snapshot,bookmark", "-o",
                 "name,guid,createtxg,creation,used,available,referenced,mountpoint",
                 "-r" if recursive else "-d", *([] if recursive else ["1"]), dataset)
    result = []
    for line in output.splitlines():
        name, guid, txg, creation, *columns = line.split("\t")
        owner = re.split("[@#]", name)[0]
        if not recursive and owner != dataset:
            continue
        result.append(dict(name=name, guid=int(guid), txg=int(txg),
                           creation=int(creation), columns=columns,
                           snapshot="@" in name))
    return result


def snapshots(dataset):
    return [v for v in versions(dataset) if v["snapshot"]]


def logical(name):
    return name.removeprefix(BACKUP_PREFIX)


def merge_snapshots(entries, conflicts=None):
    """Merge SSD and HDD copies by logical name, preferring the SSD entry.

    Conflicting GUIDs are an error unless a `conflicts` set is supplied, in
    which case the names are collected there and the SSD entry is kept.
    """
    merged = {}
    for entry in entries:
        if not entry["snapshot"]:
            continue
        name = logical(entry["name"])
        if name in merged and merged[name]["guid"] != entry["guid"]:
            if conflicts is None:
                raise ValueError(f"Conflicting SSD/HDD snapshot GUIDs: {name}")
            conflicts.add(name)
        if name not in merged or entry["name"] == name:
            merged[name] = entry
    return sorted(merged.items(), key=lambda item: (
        item[0].partition("@")[0], item[1]["creation"], item[0]))


def human_size(value):
    if not value.isdigit():
        return value
    number = int(value)
    for suffix in ("", "K", "M", "G", "T", "P", "E"):
        if number < 1024 or suffix == "E":
            if not suffix:
                return str(number)
            # Three significant digits, but never scientific notation for 1000-1023.
            return f"{number:.3g}{suffix}" if number < 1000 else f"{number:.0f}{suffix}"
        number /= 1024


def list_snapshots():
    existing = filesystems()
    entries = []
    for root in (ROOT, BACKUP_PREFIX + ROOT):
        if root in existing:
            entries.extend(versions(root, recursive=True))
        else:
            print(f"Snapshot tree unavailable: {root}", file=sys.stderr)
    # A single re-created snapshot must not hide every other world from the
    # account; mount and rollback still refuse the conflicting name itself.
    conflicts = set()
    for name, entry in merge_snapshots(entries, conflicts):
        used, available, referenced, mountpoint = entry["columns"]
        print("\t".join([name, human_size(used), human_size(available),
                         human_size(referenced), mountpoint]))
    for name in sorted(conflicts):
        print(f"Conflicting SSD/HDD snapshot GUIDs (SSD copy listed; "
              f"mount and rollback will refuse it): {name}", file=sys.stderr)


def resolve(name):
    dataset = name.partition("@")[0]
    existing = filesystems()
    entries = []
    for fs in (dataset, BACKUP_PREFIX + dataset):
        if fs in existing:
            entries.extend(v for v in snapshots(fs) if logical(v["name"]) == name)
    merged = dict(merge_snapshots(entries))
    if name not in merged:
        raise ValueError(f"Snapshot not found on SSD or HDD: {name}")
    return merged[name]


def replicated(dataset):
    matches = [(len(pattern.rstrip("<")), not pattern.endswith("<"), enabled)
               for pattern, enabled in json.loads(FILTERS).items()
               if dataset == pattern or (pattern.endswith("<") and
                   (dataset == pattern[:-1] or dataset.startswith(pattern[:-1] + "/")))]
    return max(matches, default=(0, False, False))[2]


def prop(dataset, name):
    return zfs("get", "-Hp", "-o", "value", name, dataset)


def holds(snapshot):
    return [line.split("\t")[1] for line in zfs("holds", "-H", snapshot).splitlines()]


def hold(snapshot, tag):
    if tag not in holds(snapshot):
        zfs("hold", tag, snapshot)


def release(snapshot, tag):
    if tag in holds(snapshot):
        zfs("release", tag, snapshot)


def write_state(name, data):
    temporary = STATE / (name + ".tmp")
    with temporary.open("w") as stream:
        json.dump(data, stream)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, STATE / name)
    sync_state()


def sync_state():
    fd = os.open(STATE, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def remove_state(name):
    (STATE / name).unlink(missing_ok=True)
    sync_state()


@contextmanager
def locked():
    global LOCK_FD
    info = STATE.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o077:
        raise ValueError(f"Unsafe state directory: {STATE}")
    with (STATE / "lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        LOCK_FD = lock.fileno()
        try:
            yield
        finally:
            LOCK_FD = None


def mounts(include_other=False):
    def unescape(value):
        return re.sub(r"\\([0-7]{3})", lambda m: chr(int(m[1], 8)), value)
    result = []
    for line in Path("/proc/self/mountinfo").read_text().splitlines():
        left, right = line.split(" - ", 1)
        fields, filesystem = left.split(), right.split()
        if filesystem[0] == "zfs" or include_other:
            result.append(dict(source=unescape(filesystem[1]),
                               target=unescape(fields[4]), root=unescape(fields[3]),
                               options=fields[5], fstype=filesystem[0]))
    return result


def safe_directory(path):
    """Pin a directory without following any role-account-controlled symlinks."""
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in Path(path).parts[1:]:
            next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                              dir_fd=fd)
            os.close(fd)
            fd = next_fd
        return fd
    except BaseException:
        os.close(fd)
        raise


def restore_mounts(recorded):
    for mount in sorted(recorded, key=lambda m: m["target"].count("/")):
        current = [m for m in mounts(include_other=True) if m["target"] == mount["target"]]
        if current:
            if len(current) == 1 and current[0]["source"] == mount["source"] and current[0]["fstype"] == "zfs":
                continue
            raise ValueError(f"Unexpected mount at {mount['target']}")
        fd = safe_directory(mount["target"])
        try:
            mount_pinned(mount, fd)
        finally:
            os.close(fd)


def mount_flags(options):
    """Decode only per-mount VFS flags, never ZFS properties or helper options."""
    flags_by_name = {"rw": 0, "ro": 1, "nosuid": 2, "nodev": 4, "noexec": 8,
                     "sync": 16, "dirsync": 128, "noatime": 1024,
                     "nodiratime": 2048, "relatime": 1 << 21,
                     "strictatime": 1 << 24, "lazytime": 1 << 25}
    flags = 0
    for option in options.split(","):
        if option not in flags_by_name:
            raise ValueError(f"Unsupported mount flag: {option}")
        flags |= flags_by_name[option]
    return flags


def mount_pinned(mount, fd):
    # Use mount(2) directly: libmount may reopen/canonicalize an fd target.
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mount.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p,
                           ctypes.c_ulong, ctypes.c_char_p]
    libc.mount.restype = ctypes.c_int
    if libc.mount(os.fsencode(mount["source"]), f"/proc/self/fd/{fd}".encode(),
                  b"zfs", mount_flags(mount["options"]), None) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), mount["target"])


def clean_mount_hold():
    state = STATE / "mount.json"
    if state.exists():
        saved = json.loads(state.read_text())
        if any(m["source"] == saved["name"] for m in mounts()):
            return
        # The dataset may have been renamed or destroyed since the mount was
        # recorded; a stale record must not block every privileged command.
        dataset = saved["name"].partition("@")[0]
        if dataset in filesystems() and any(
                v["name"] == saved["name"] and v["guid"] == saved["guid"]
                for v in snapshots(dataset)):
            release(saved["name"], MOUNT_HOLD)
        remove_state("mount.json")


def mount_snapshot(name):
    if any(m["target"] == MOUNTPOINT for m in mounts()) or os.path.ismount(MOUNTPOINT):
        raise ValueError(f"Already mounted: {MOUNTPOINT}")
    entry = resolve(name)
    write_state("mount.json", entry)
    hold(entry["name"], MOUNT_HOLD)
    try:
        run("@mount@", "-i", "-t", "zfs", "-o", "ro,nosuid,nodev,noexec",
            "--source", entry["name"], "--target", MOUNTPOINT)
    finally:
        clean_mount_hold()


def send_receive(source, destination, base=None, force=True):
    args = ["@zfs@", "send"]
    if base:
        args.extend(["-i", base])
    args.append(source)
    with subprocess.Popen(args, stdout=subprocess.PIPE, env=ENV, pass_fds=child_fds()) as sender:
        try:
            receiver = subprocess.run(
                ["@zfs@", "receive", "-u", *(["-F"] if force else []), destination],
                stdin=sender.stdout, env=ENV, check=False, pass_fds=child_fds())
        finally:
            sender.stdout.close()
            # A failed receive must not leave a producer blocked on a full pipe.
            sender_status = sender.wait()
        if receiver.returncode or sender_status:
            raise ValueError(f"ZFS transfer failed (send={sender_status}, receive={receiver.returncode})")


def choose_restore(name, live, backup, is_replicated):
    """Pure restore planning: never compare TXGs across pools."""
    merged = dict(merge_snapshots(live + backup))
    if name not in merged:
        raise ValueError(f"Snapshot not found: {name}")
    target = merged[name]
    local_target = next((v for v in live if v["snapshot"] and v["name"] == name), None)
    receiver_target = next((v for v in backup if v["snapshot"] and
                            v["guid"] == target["guid"]), None)
    result = dict(name=name, live=name.partition("@")[0], target=target,
                  backup=BACKUP_PREFIX + name.partition("@")[0] if is_replicated else None,
                  base=None, anchor=None, method="local")
    if not is_replicated:
        if local_target is None:
            raise ValueError("Cannot restore an HDD-only snapshot into an excluded dataset")
    elif not backup:
        raise ValueError("Replica has no history; replicate this dataset before rolling back")
    if local_target:
        if is_replicated:
            surviving = {v["guid"]: v for v in live if v["txg"] <= local_target["txg"]}
            common = [v for v in backup if v["snapshot"] and v["guid"] in surviving]
            if not common:
                raise ValueError("No surviving common replication base; refusing to erase backup history")
            anchor = max(common, key=lambda v: v["txg"])
            result["anchor"] = dict(sender=surviving[anchor["guid"]], receiver=anchor)
        result["remove_live"] = [v for v in live if v["txg"] > local_target["txg"]]
    else:
        live_guids = {v["guid"]: v for v in live if v["snapshot"]}
        common = [v for v in backup if v["snapshot"] and
                  v["txg"] < receiver_target["txg"] and v["guid"] in live_guids]
        if common:
            base = max(common, key=lambda v: v["txg"])
            result["base"] = dict(sender=base, receiver=live_guids[base["guid"]])
            result["method"] = "incremental"
            result["remove_live"] = [v for v in live if v["txg"] > live_guids[base["guid"]]["txg"]]
        else:
            if any(v["snapshot"] and v["creation"] <= target["creation"] for v in live):
                raise ValueError("Full receive would erase older SSD-only history")
            result["method"] = "full"
            result["remove_live"] = [v for v in live if v["snapshot"] or
                                     v["creation"] > target["creation"]]
        restored = dict(target, name=name)
        result["anchor"] = dict(sender=restored, receiver=receiver_target)
    result["remove_backup"] = [v for v in backup if result["anchor"] and
                               v["txg"] > result["anchor"]["receiver"]["txg"]]
    return result


def preflight(name):
    dataset = name.partition("@")[0]
    existing = filesystems()
    if dataset not in existing:
        raise ValueError(f"Live dataset does not exist: {dataset}")
    is_replicated = replicated(dataset)
    backup = BACKUP_PREFIX + dataset
    if is_replicated and backup not in existing:
        raise ValueError(f"Replica unavailable: {backup}; refusing an uncoordinated rollback")
    live = versions(dataset)
    remote = versions(backup) if is_replicated else []
    plan = choose_restore(name, live, remote, is_replicated)
    if prop(dataset, "receive_resume_token") != "-":
        raise ValueError("Live dataset has an unfinished receive; administrator recovery required")
    plan["abort_receive"] = is_replicated and prop(backup, "receive_resume_token") != "-"
    for entry in plan["remove_live"] + plan["remove_backup"]:
        if not entry["snapshot"]:
            continue
        allowed = {SENDER_HOLD} if entry["name"].startswith(ROOT) else {RECEIVER_HOLD}
        foreign = set(holds(entry["name"])) - allowed
        clones = set(prop(entry["name"], "clones").split(",")) - {"-"}
        # A resumable receive keeps an internal clone of its base. The exact
        # %recv clone disappears when we abort this dataset's receive token.
        if plan["abort_receive"]:
            clones.discard(backup + "/%recv")
        if foreign or clones:
            raise ValueError(f"Snapshot has blocking holds or clones: {entry['name']} "
                             f"(holds={sorted(foreign)}, clones={sorted(clones)})")
        if any(m["source"] == entry["name"] for m in mounts()):
            raise ValueError(f"Snapshot is mounted: {entry['name']}")
    plan["mounts"] = []
    if plan["method"] == "full":
        if prop(dataset, "encryption") != "off" or prop(dataset, "origin") != "-":
            raise ValueError("Full receive requires an unencrypted, non-clone live dataset")
        for mount in mounts():
            if mount["source"] == dataset or mount["source"].startswith(dataset + "/"):
                expected = "/home/minecraft" + mount["source"][len(ROOT):]
                if mount["root"] != "/" or mount["target"] != expected:
                    raise ValueError(f"Unsupported live mount layout: {mount['target']}")
                mount_flags(mount["options"])
                fd = safe_directory(mount["target"])
                os.close(fd)
                plan["mounts"].append(mount)
        if len({m["target"] for m in plan["mounts"]}) != len(plan["mounts"]):
            raise ValueError("Stacked live mounts are not supported")
    if plan["method"] != "local":
        args = ["send", "-nP"]
        if plan["base"]:
            args.extend(["-i", plan["base"]["sender"]["name"]])
        zfs(*args, plan["target"]["name"])
    plan["protect"] = [plan["target"]]
    if plan["base"]:
        plan["protect"].extend(plan["base"].values())
    if plan["anchor"]:
        plan["protect"].extend(v for v in plan["anchor"].values()
                               if v["snapshot"] and v["name"] != name)
    # The restored SSD target doesn't exist yet. All other protection entries do.
    plan["protect"] = list({v["name"]: v for v in plan["protect"]}.values())
    plan["phase"] = "restore"
    plan["version"] = 1
    return plan


def verify_entry(entry):
    fs = re.split("[@#]", entry["name"])[0]
    match = next((v for v in versions(fs) if v["name"] == entry["name"]), None)
    if match is None or match["guid"] != entry["guid"]:
        raise ValueError(f"Recovery snapshot/bookmark changed or disappeared: {entry['name']}")


def remove_planned(entries):
    """Remove individual versions only, preserving children and foreign holds."""
    for entry in entries:
        fs = re.split("[@#]", entry["name"])[0]
        current = next((v for v in versions(fs) if v["name"] == entry["name"]), None)
        if current is None:
            continue
        verify_entry(entry)
        if entry["snapshot"]:
            allowed = SENDER_HOLD if entry["name"].startswith(ROOT) else RECEIVER_HOLD
            if set(holds(entry["name"])) - {allowed}:
                raise ValueError(f"Unexpected hold on {entry['name']}")
            release(entry["name"], allowed)
        zfs("destroy", entry["name"])


def repair_metadata(plan):
    anchor = plan["anchor"]
    if not anchor:
        return
    sender, receiver = anchor["sender"], anchor["receiver"]
    # A previous attempt may have replaced a tentative cursor with the final
    # cursor before interruption. The GUID, not that obsolete name, is stable.
    sender = next((v for v in versions(plan["live"]) if v["guid"] == sender["guid"]), None)
    if sender is None:
        raise ValueError("The surviving sender replication base disappeared")
    verify_entry(receiver)
    cursor = f"{plan['live']}#zrepl_CURSOR_G_{sender['guid']:016x}_J_rpool"
    current = next((v for v in versions(plan["live"]) if v["name"] == cursor), None)
    if current:
        verify_entry(dict(sender, name=cursor))
    else:
        zfs("bookmark", sender["name"], cursor)
    hold(receiver["name"], RECEIVER_HOLD)
    for entry in versions(plan["live"]):
        if not entry["snapshot"] and MANAGED_BOOKMARK.fullmatch(entry["name"].partition("#")[2]):
            if entry["name"] != cursor:
                zfs("destroy", entry["name"])
        elif entry["snapshot"]:
            release(entry["name"], SENDER_HOLD)
    for entry in snapshots(plan["backup"]):
        if entry["name"] != receiver["name"]:
            release(entry["name"], RECEIVER_HOLD)


def resume_rollback(plan):
    for entry in plan["protect"]:
        verify_entry(entry)
        hold(entry["name"], ROLLBACK_HOLD)
    if plan["phase"] != "restore":
        verify_entry(dict(plan["target"], name=plan["name"]))
        if plan["anchor"] and not any(v["guid"] == plan["anchor"]["sender"]["guid"]
                                      for v in versions(plan["live"])):
            raise ValueError("The surviving sender replication base disappeared")
    if plan["abort_receive"]:
        if prop(plan["backup"], "receive_resume_token") != "-":
            zfs("receive", "-A", plan["backup"])
    if plan["phase"] == "restore":
        restored = next((v for v in snapshots(plan["live"]) if v["name"] == plan["name"]), None)
        if restored and restored["guid"] != plan["target"]["guid"]:
            raise ValueError("Live target GUID changed during recovery")
        if plan["method"] == "full":
            for mount in sorted(plan["mounts"], key=lambda m: -m["target"].count("/")):
                if any(m["target"] == mount["target"] and m["source"] == mount["source"] for m in mounts()):
                    run("@umount@", "--", mount["target"])
        if not restored or plan["method"] == "local":
            remove_planned(plan["remove_live"])
            if plan["method"] == "local":
                zfs("rollback", "-r", plan["name"])
            else:
                base = plan["base"]
                if base:
                    verify_entry(base["receiver"])
                    zfs("rollback", "-r", base["receiver"]["name"])
                send_receive(plan["target"]["name"], plan["live"],
                             base["sender"]["name"] if base else None)
        verify_entry(dict(plan["target"], name=plan["name"]))
        plan["phase"] = "replica"
        write_state("rollback.json", plan)
    if plan["phase"] == "replica":
        if plan["backup"]:
            remove_planned(plan["remove_backup"])
            zfs("rollback", "-r", plan["anchor"]["receiver"]["name"])
        plan["phase"] = "metadata"
        write_state("rollback.json", plan)
    if plan["phase"] == "metadata":
        repair_metadata(plan)
        plan["phase"] = "mounts"
        write_state("rollback.json", plan)
    restore_mounts(plan["mounts"])
    for entry in plan["protect"]:
        release(entry["name"], ROLLBACK_HOLD)
    # Keep a separate completion record so interruption while restarting the
    # daemon cannot lose the administrator's original service state.
    write_state("complete.json", dict(was_active=plan["was_active"], wake=bool(plan["backup"])))
    remove_state("rollback.json")
    finish_restart()


def finish_restart():
    path = STATE / "complete.json"
    if not path.exists() or (STATE / "rollback.json").exists():
        return
    saved = json.loads(path.read_text())
    if saved["was_active"]:
        run("@systemctl@", "start", "zrepl.service")
        if saved["wake"]:
            deadline = time.monotonic() + 10
            while True:
                result = run("@zrepl@", "signal", "wakeup", "rpool", capture=True,
                             stderr=subprocess.PIPE, check=False)
                if result.returncode == 0:
                    break
                if time.monotonic() >= deadline:
                    raise ValueError(f"Could not wake zrepl; restart remains pending: {result.stderr.strip()}")
                time.sleep(0.1)
    remove_state("complete.json")


def rollback(name):
    path = STATE / "rollback.json"
    if path.exists():
        plan = json.loads(path.read_text())
        if plan.get("version") != 1:
            raise ValueError("Unsupported rollback journal version; use the helper that created it")
        if plan["name"] != name:
            raise ValueError(f"Recovery pending; repeat: minecraft-storage rollback {plan['name']}")
        run("@systemctl@", "stop", "zrepl.service")
    else:
        active = run("@systemctl@", "is-active", "--quiet", "zrepl.service", check=False).returncode == 0
        # Persist prior service state before stopping, including interruption in preflight.
        write_state("complete.json", dict(was_active=active, wake=False))
        run("@systemctl@", "stop", "zrepl.service")
        try:
            plan = preflight(name)
            plan["was_active"] = active
            write_state("rollback.json", plan)
        except BaseException:
            finish_restart()
            raise
    try:
        resume_rollback(plan)
    except BaseException:
        if path.exists():
            print(f"Rollback incomplete; zrepl remains stopped. Keep the server stopped and repeat:\n"
                  f"  minecraft-storage rollback {name}", file=sys.stderr)
        else:
            print("Rollback completed, but zrepl restart is pending. Repeat the command to retry.",
                  file=sys.stderr)
        raise


def interrupted(signum, frame):
    raise InterruptedError(f"Interrupted by signal {signum}")


def main():
    os.chdir("/")
    try:
        operation, name = command(sys.argv[1:])
        if operation in ("status", "pools"):
            run("@zpool@", "status" if operation == "status" else "list")
        elif operation == "snapshots":
            list_snapshots()
        else:
            for signum in (signal.SIGTERM, signal.SIGHUP):
                signal.signal(signum, interrupted)
            with locked():
                finish_restart()
                if (STATE / "rollback.json").exists() and operation != "rollback":
                    raise ValueError("Rollback recovery pending; repeat the interrupted rollback command")
                clean_mount_hold()
                if operation == "rollback":
                    rollback(name)
                elif operation == "snapshot":
                    zfs("snapshot", name)
                elif operation == "mount":
                    mount_snapshot(name)
                elif operation == "unmount":
                    run("@umount@", "--", MOUNTPOINT)
                    clean_mount_hold()
    except (ValueError, OSError, subprocess.CalledProcessError, KeyboardInterrupt) as error:
        sys.exit(str(error) or "Interrupted")


if __name__ == "__main__":
    main()
