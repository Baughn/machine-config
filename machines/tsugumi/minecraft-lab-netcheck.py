"""minecraft-lab-netcheck: notice when the lab's network namespace loses the internet, and heal it.

The lab reaches the internet only through pasta (minecraft-lab-pasta). On 2026-10-03
the namespace went deaf while pasta kept running: the lab agent's DNS lookups failed
from 12:49 UTC (or earlier) until pasta was restarted, cause unknown. Runs as root from a timer.
It probes from inside the namespace with a raw DNS query (as c-ares/aiodns do; glibc
would go through the host's nscd and hide the problem) and a TCP connect. After two
failures in a row it logs diagnostics, restarts pasta (which takes the lab agent's
bridge with it) unless a lab server is running, and tells the channel through
discord-notify. See minecraft-lab.nix.

    minecraft-lab-netcheck NETNS STATE_DIR
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import time

FAILURES_BEFORE_ACTION = 2
RESTART_BACKOFF = 30 * 60

# Runs inside the namespace: a DNS query for discord.com to 1.1.1.1 over UDP, and a TCP
# connect to 1.1.1.1:443. Prints one JSON line.
PROBE = r"""
import json, socket, struct, time
result = {}
query = struct.pack(">HHHHHH", 0x1234, 0x0100, 1, 0, 0, 0)
query += b"".join(bytes([len(p)]) + p for p in (b"discord", b"com")) + b"\0"
query += struct.pack(">HH", 1, 1)
for attempt in range(2):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(3)
    try:
        start = time.monotonic()
        s.sendto(query, ("1.1.1.1", 53))
        reply = s.recv(512)
        result["dns"] = reply[:2] == b"\x12\x34"
        result["dns_ms"] = round((time.monotonic() - start) * 1000)
        break
    except OSError as error:
        result["dns"] = False
        result["dns_error"] = str(error)
    finally:
        s.close()
try:
    socket.create_connection(("1.1.1.1", 443), timeout=5).close()
    result["tcp"] = True
except OSError as error:
    result["tcp"] = False
    result["tcp_error"] = str(error)
print(json.dumps(result))
"""


def run(*args, timeout=30):
    try:
        done = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
        return (done.stdout + done.stderr).strip()
    except (OSError, subprocess.TimeoutExpired) as error:
        return f"{args[0]}: {error}"


def probe(netns=None):
    """In the lab's namespace, or on the host when netns is None."""
    command = [sys.executable, "-I", "-c", PROBE]
    if netns:
        command = ["nsenter", f"--net={netns}"] + command
    out = run(*command, timeout=20)
    try:
        return json.loads(out.splitlines()[-1])
    except (ValueError, IndexError):
        return {"dns": False, "tcp": False, "probe_error": out[-300:]}


def healthy(result):
    return bool(result.get("dns")) and bool(result.get("tcp"))


def diagnostics(netns):
    """What might explain it, for the journal (Baughn wants to know why this happens)."""
    pid = run("systemctl", "show", "-P", "MainPID", "minecraft-lab-pasta.service")
    lines = [
        "ns addr: " + run("nsenter", f"--net={netns}", "ip", "-4", "-o", "addr"),
        "ns route: " + run("nsenter", f"--net={netns}", "ip", "-4", "route"),
        "host route: " + run("ip", "-4", "route", "show", "default"),
        "pasta: " + run("systemctl", "show", "-p", "ActiveEnterTimestamp,MemoryCurrent,NRestarts",
                        "minecraft-lab-pasta.service").replace("\n", " "),
    ]
    if pid.isdigit() and pid != "0":
        try:
            lines.append(f"pasta fds: {len(os.listdir(f'/proc/{pid}/fd'))}")
        except OSError as error:
            lines.append(f"pasta fds: {error}")
        sockets = run("ss", "-H", "-tuanp").count(f"pid={pid},")
        lines.append(f"pasta sockets: {sockets}")
    lines.append("fence: " + run("nft", "list", "table", "inet", "mclab-fence").replace("\n", " ")[:400])
    return lines


def notify(message):
    run("discord-notify", "--name", "Lab network", message)


def lab_servers():
    out = run("systemctl", "list-units", "--plain", "--no-legend", "--state=active", "minecraft-lab@*")
    return [line.split()[0] for line in out.splitlines() if line.strip()]


def main():
    netns, state_dir = sys.argv[1], Path(sys.argv[2])
    state_file = state_dir / "state.json"
    try:
        state = json.loads(state_file.read_text())
    except (OSError, ValueError):
        state = {}
    failures = state.get("failures", 0)

    result = probe(netns)
    if healthy(result):
        if failures >= FAILURES_BEFORE_ACTION:
            notify("The lab's network works again.")
        state = {"failures": 0, "last_restart": state.get("last_restart", 0)}
        state_file.write_text(json.dumps(state))
        return

    host = probe()
    if not healthy(host):
        # The host itself is offline: not pasta's fault, and a restart wouldn't help.
        print(f"lab probe failed, but so does the host's: {json.dumps(host)}")
        return

    failures += 1
    print(f"probe failed ({failures} in a row): {json.dumps(result)}")
    state["failures"] = failures
    if failures < FAILURES_BEFORE_ACTION:
        state_file.write_text(json.dumps(state))
        return

    for line in diagnostics(netns):
        print(line)
    servers = lab_servers()
    since_restart = time.time() - state.get("last_restart", 0)
    if servers:
        if failures == FAILURES_BEFORE_ACTION:
            notify(f"The lab's network namespace has lost the internet ({json.dumps(result)}), "
                   f"but {', '.join(servers)} is running, so I'm not restarting pasta. "
                   "Stop it, and the next check heals the network.")
    elif since_restart < RESTART_BACKOFF:
        if failures == FAILURES_BEFORE_ACTION:
            notify("The lab's network is down again within 30 minutes of a pasta restart; "
                   "not restarting again until then. Diagnostics are in "
                   "`journalctl -u minecraft-lab-netcheck`.")
    else:
        print("restarting minecraft-lab-pasta")
        run("systemctl", "restart", "minecraft-lab-pasta.service", timeout=120)
        run("systemctl", "start", "agent-bridge-tsugumi-lab.service", timeout=60)
        time.sleep(5)
        after = probe(netns)
        state["last_restart"] = time.time()
        if healthy(after):
            state["failures"] = 0
            notify(f"The lab's network namespace had lost the internet ({json.dumps(result)}). "
                   "I restarted pasta, and it works again. LabAgent's bridge restarted with it. "
                   "Diagnostics: `journalctl -u minecraft-lab-netcheck`.")
        else:
            notify(f"The lab's network namespace has lost the internet; restarting pasta didn't "
                   f"help ({json.dumps(after)}). Diagnostics: `journalctl -u minecraft-lab-netcheck`.")
    state_file.write_text(json.dumps(state))


if __name__ == "__main__":
    main()
