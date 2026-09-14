"""Best-effort Minecraft save coordination for zrepl snapshots."""

from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import socket
import struct
import sys
import time

import javaproperties


WORLDS = Path("/home/minecraft")
STATE = Path("/run/minecraft-save-hook")
TCP_TABLES = (Path("/proc/net/tcp"), Path("/proc/net/tcp6"))
# Longer than zrepl's two-minute hook timeout; the independent timer retries
# save-on even when zrepl kills the hook or disappears between its two edges.
RECOVERY_DELAY = 180


class Rcon:
    def __init__(self, sock):
        self.sock = sock
        self.sequence = 0

    def request(self, kind, text, timeout=40):
        self.sequence += 1
        deadline = time.monotonic() + timeout
        body = struct.pack("<ii", self.sequence, kind) + text.encode() + b"\0\0"
        self.sock.settimeout(timeout)
        self.sock.sendall(struct.pack("<i", len(body)) + body)

        def read(size):
            result = b""
            while len(result) < size:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("RCON response timed out")
                self.sock.settimeout(remaining)
                part = self.sock.recv(size - len(result))
                if not part:
                    raise ConnectionError("RCON closed before replying")
                result += part
            return result

        size, = struct.unpack("<i", read(4))
        if not 10 <= size <= 4096 + 10:
            raise ValueError("Invalid RCON packet length")
        packet = read(size)
        sequence, response_kind = struct.unpack("<ii", packet[:8])
        if sequence != self.sequence or response_kind != (2 if kind == 3 else 0):
            raise ValueError("RCON authentication failed or unexpected response")
        if packet[-2:] != b"\0\0":
            raise ValueError("Invalid RCON packet terminator")
        return packet[8:-2].decode("utf-8")

    def command(self, text, timeout=40):
        return self.request(2, text, timeout)


def port_listening(port):
    # Local firewall rules can silently drop connections to closed ports.
    # Check both families: Java commonly uses a dual-stack IPv6 listener.
    for path in TCP_TABLES:
        if path.name == "tcp6" and not path.exists():
            continue
        for line in path.read_text().splitlines()[1:]:
            fields = line.split()
            if fields[3] == "0A" and int(fields[1].rsplit(":", 1)[1], 16) == port:
                return True
    return False


@contextmanager
def connect(world, timeout=40):
    # Read as minecraft at runtime: credentials never enter the Nix store,
    # process arguments, shell interpolation, or hook logs.
    with (world / "server.properties").open("rb") as handle:
        properties = javaproperties.load(handle)
    password = properties.get("rcon.password", "")
    port = int(properties.get("rcon.port", "25575"))
    if not 1 <= port <= 65535:
        raise ValueError("Invalid RCON port")
    if not port_listening(port):
        raise ConnectionRefusedError("No local RCON listener")
    with socket.create_connection(("127.0.0.1", port), timeout=3) as sock:
        if properties.get("enable-rcon") != "true" or not password:
            raise ValueError("RCON must be enabled with a nonempty password")
        client = Rcon(sock)
        client.request(3, password, min(timeout, 5))
        yield client


class SnapshotHook:
    def __init__(self, name):
        if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.:-]*", name) is None:
            raise ValueError("Invalid Minecraft instance name")
        self.world = WORLDS / name
        self.state = STATE / name
        self.record = self.state / "pending.json"

    def pending(self):
        return json.loads(self.record.read_text()) if self.record.exists() else None

    def restore(self):
        try:
            with connect(self.world, timeout=10) as client:
                reply = client.command("save-on", timeout=10)
            if reply not in ("Turned on world auto-saving", "Saving is already turned on"):
                raise ValueError("Minecraft did not confirm save-on")
            print(f"{self.world.name}: automatic saving restored", flush=True)
        except ConnectionRefusedError:
            # A closed RCON port means the server is off. save-off does not
            # survive a restart, so there is no state to recover in that case.
            pass
        self.record.unlink()

    def pre_snapshot(self, snapshot):
        if self.pending():
            self.restore()
        try:
            with connect(self.world) as client:
                record = {"snapshot": snapshot, "deadline": time.monotonic() + RECOVERY_DELAY}
                temporary = self.state / "pending.tmp"
                temporary.write_text(json.dumps(record))
                temporary.replace(self.record)
                try:
                    reply = client.command("save-off")
                    if reply == "Saving is already turned off":
                        # Do not take ownership of an administrator's save-off state.
                        self.record.unlink()
                        raise ValueError("Saving was already disabled; snapshot skipped")
                    if reply != "Turned off world auto-saving":
                        raise ValueError("Minecraft did not confirm save-off")
                    reply = client.command("save-all flush")
                    if ("Flushing completed" not in reply or "Saved the world" not in reply
                            or "Saving failed" in reply):
                        raise ValueError("Minecraft did not confirm a completed save-all flush")
                    print(f"{self.world.name}: save-all flush completed; ready for snapshot", flush=True)
                except Exception:
                    if self.pending():
                        try:
                            self.restore()
                        except Exception as error:
                            print(f"save-on recovery pending: {error}", file=sys.stderr)
                    raise
        except ConnectionRefusedError:
            print(f"{self.world.name}: RCON port closed; snapshot needs no save", flush=True)

    def run(self, action, snapshot=""):
        self.state.mkdir(mode=0o700, exist_ok=True)
        with (self.state / "lock").open("a") as lock:
            # A recovery timer must never interrupt a save in progress, nor
            # let one jammed instance prevent recovery of the other servers.
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                if action == "recover":
                    return
                raise
            if action == "pre_snapshot":
                self.pre_snapshot(snapshot)
                return
            record = self.pending()
            if record is None:
                return
            if action == "post_snapshot":
                if record["snapshot"] != snapshot:
                    raise ValueError("Post-snapshot hook does not match pending save")
            elif action == "recover":
                if time.monotonic() < record["deadline"]:
                    return
                print("Snapshot save lease expired; recovering automatic saving", file=sys.stderr)
            else:
                raise ValueError("Unknown hook action")
            self.restore()


def main():
    if sys.argv[1:] == ["recover"]:
        failed = False
        for path in sorted(STATE.iterdir()):
            if path.is_dir():
                try:
                    SnapshotHook(path.name).run("recover")
                except Exception as error:
                    print(f"{path.name}: recovery failed: {error}", file=sys.stderr)
                    failed = True
        if failed:
            raise ValueError("Minecraft save recovery remains pending")
        return
    dataset = os.environ.get("ZREPL_FS", "")
    if sys.argv[1:] or not dataset.startswith("rpool/minecraft"):
        raise ValueError("Expected a Minecraft zrepl snapshot hook invocation")
    action = os.environ.get("ZREPL_HOOKTYPE")
    if action not in ("pre_snapshot", "post_snapshot"):
        raise ValueError("Unknown zrepl hook type")
    if os.environ.get("ZREPL_DRYRUN") == "true":
        print(f"Would run Minecraft {action}")
        return
    # Only the direct children are server roots; parent and nested auxiliary
    # datasets (e.g. dynmap) do not get independent save cycles.
    parts = dataset.split("/")
    if parts[:2] != ["rpool", "minecraft"]:
        raise ValueError("Unexpected dataset")
    if len(parts) != 3:
        return
    SnapshotHook(parts[2]).run(action, os.environ["ZREPL_SNAPNAME"])


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"minecraft-snapshot: {error}", file=sys.stderr)
        sys.exit(1)
