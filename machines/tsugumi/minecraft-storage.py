"""Narrow sudo interface for Minecraft's ZFS snapshots."""

import os
import re
import sys

MOUNTPOINT = "/run/minecraft-snapshot"
SNAPSHOT = re.compile(
    r"rpool/minecraft(?:/[A-Za-z0-9_][A-Za-z0-9_.:-]*)*"
    r"@[A-Za-z0-9_][A-Za-z0-9_.:-]*\Z"
)


def command(arguments):
    """Return a fixed argv, rejecting extra flags and unrelated datasets."""
    match arguments:
        case ["status"]:
            return ["@zpool@", "status"]
        case ["pools"]:
            return ["@zpool@", "list"]
        case ["snapshots"]:
            return ["@zfs@", "list", "-t", "snapshot", "-H", "-r", "rpool/minecraft"]
        case ["unmount"]:
            return ["@umount@", "--", MOUNTPOINT]
        case [operation, snapshot] if operation in ("snapshot", "rollback", "mount"):
            if len(snapshot) > 255 or SNAPSHOT.fullmatch(snapshot) is None:
                raise ValueError("Expected a snapshot beneath rpool/minecraft")
            if operation == "snapshot":
                return ["@zfs@", "snapshot", snapshot]
            if operation == "rollback":
                return ["@zfs@", "rollback", "-r", snapshot]
            return [
                "@mount@", "-t", "zfs", "-o", "ro,nosuid,nodev,noexec",
                "--source", snapshot, "--target", MOUNTPOINT,
            ]
        case _:
            raise ValueError(
                "Usage: minecraft-storage status|pools|snapshots|unmount "
                "or snapshot|rollback|mount DATASET@SNAPSHOT"
            )


def main():
    try:
        argv = command(sys.argv[1:])
    except ValueError as error:
        sys.exit(str(error))
    # Ignore the invoking user's working directory and environment, including
    # ZFS helper-script settings. The Python interpreter also runs with -I.
    os.chdir("/")
    os.execve(argv[0], argv, {"PATH": "@path@", "LANG": "C.UTF-8"})


if __name__ == "__main__":
    main()
