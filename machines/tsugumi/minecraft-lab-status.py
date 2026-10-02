"""Posts the lab's status card to the agent board: clones and running lab servers.

Runs as the lab user from a timer; the board knows that uid as tsugumi-lab and shows
the card on agents.brage.info/status. Usage: minecraft-lab-status BOARD_SOCKET
"""

import http.client
import json
import socket
import subprocess
import sys
import time

TTL = 180  # The timer runs every minute; three missed runs show the card as stale.
GIB = 1024 ** 3


class UnixConnection(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost", timeout=10)
        self.path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(10)
        self.sock.connect(self.path)


def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=False).stdout


def clones():
    """(lines, count) from `minecraft-lab list` (tab-separated, one clone per line)."""
    result = subprocess.run(["minecraft-lab", "list"], capture_output=True, text=True, check=False)
    if result.returncode != 0:
        return [{"text": f"minecraft-lab list failed: {result.stderr.strip()[:200]}", "level": "alert"}], 0
    lines = []
    for row in result.stdout.splitlines():
        fields = row.split("\t")
        if len(fields) < 5:
            continue  # the closing "N of M lab clones" line
        name, origin, age, expires, size = fields[:5]
        snapshot = origin.rpartition("/")[2]
        level = None
        if expires == "expired":
            level = "alert"
        elif " d " not in expires:
            level = "warn"  # less than a day left
        lines.append({"text": f"clone {name}: {expires}, {age}, {size}, from {snapshot}",
                      **({"level": level} if level else {})})
    return lines, len(lines)


def show(units, properties):
    """systemctl show for units, as a list of {property: value}."""
    if not units:
        return []
    output = run("systemctl", "show", "--timestamp=unix", "--property=" + ",".join(properties), *units)
    return [dict(line.split("=", 1) for line in block.splitlines() if "=" in line)
            for block in output.strip().split("\n\n") if block.strip()]


def memory(value):
    return int(value) / GIB if value.isdigit() else None


def servers(now):
    units = run("systemctl", "list-units", "--all", "--plain", "--no-legend", "--type=service",
                "minecraft-lab@*").split()
    units = [unit for unit in units if unit.startswith("minecraft-lab@") and unit.endswith(".service")]
    lines, running = [], 0
    for unit in show(units, ["Id", "ActiveState", "SubState", "ActiveEnterTimestamp", "MemoryCurrent"]):
        state = unit.get("ActiveState", "?")
        name = unit.get("Id", "?").removeprefix("minecraft-lab@").removesuffix(".service")
        if state == "inactive":
            continue
        if state == "failed":
            lines.append({"text": f"server {name}: failed", "level": "alert"})
            continue
        running += 1
        text = f"server {name}: {state}"
        since = unit.get("ActiveEnterTimestamp", "").lstrip("@")
        if since.isdigit():
            minutes = (now - int(since)) // 60
            text += f" for {minutes // 60} h {minutes % 60} min" if minutes >= 60 else f" for {minutes} min"
        used = memory(unit.get("MemoryCurrent", ""))
        if used is not None:
            text += f", {used:.1f} GiB"
        lines.append({"text": text})
    return lines, running


def main():
    now = int(time.time())
    server_lines, running = servers(now)
    clone_lines, count = clones()
    lines = server_lines + clone_lines
    slice_ = show(["mclab.slice"], ["MemoryCurrent", "MemoryMax"])
    if running and slice_:
        used, cap = memory(slice_[0].get("MemoryCurrent", "")), memory(slice_[0].get("MemoryMax", ""))
        if used is not None and cap:
            lines.append({"text": f"lab memory {used:.1f} of {cap:.0f} GiB",
                          **({"level": "warn"} if used > 0.85 * cap else {})})
    levels = {line.get("level") for line in lines}
    card = {
        "title": "Lab (timer)",
        "state": f"{running} server{'s' * (running != 1)} running, {count} clone{'s' * (count != 1)}",
        "level": "alert" if "alert" in levels else "warn" if "warn" in levels else
                 "info" if running else "ok",
        "lines": lines[:50],
        "ttl": TTL,
    }
    connection = UnixConnection(sys.argv[1])
    connection.request("PUT", "/status/lab", json.dumps(card), {"Content-Type": "application/json"})
    response = connection.getresponse()
    body = response.read().decode(errors="replace")
    if response.status != 200:
        sys.exit(f"board answered {response.status}: {body}")


if __name__ == "__main__":
    main()
