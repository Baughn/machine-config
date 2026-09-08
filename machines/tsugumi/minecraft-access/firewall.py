"""Private grant broker and atomic nftables reconciliation."""

import argparse
import asyncio
import fcntl
import ipaddress
import json
import logging
import math
import os
import sqlite3
import subprocess
import time
from contextlib import closing
from pathlib import Path

from aiohttp import web

logger = logging.getLogger(__name__)


class Grants:
    def __init__(self, config, directory, nft):
        self.config = config
        self.directory = Path(directory)
        self.nft = nft
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)

    def run(self, user=None, address=None, now=None):
        now = time.time() if now is None else now
        # The firewall reload hook and HTTP broker may run concurrently.
        with (self.directory / "lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            with closing(sqlite3.connect(self.directory / "grants.sqlite")) as db, db:
                db.execute(
                    "CREATE TABLE IF NOT EXISTS grants (user TEXT, ip TEXT, expiry REAL, "
                    "PRIMARY KEY(user, ip))"
                )
                db.execute("CREATE INDEX IF NOT EXISTS grants_expiry ON grants(expiry)")
                db.execute("DELETE FROM grants WHERE expiry <= ?", (now,))
                if user is not None and (
                    not isinstance(user, str)
                    or not user.isascii()
                    or not user.isdigit()
                    or len(user) > 20
                ):
                    raise ValueError("Invalid account")
                if address is not None:
                    if "%" in address:
                        raise ValueError("Scoped addresses are not supported")
                    address = str(ipaddress.ip_address(address))
                    previous = db.execute(
                        "SELECT expiry FROM grants WHERE user=? AND ip=?",
                        (user, address),
                    ).fetchone()
                    count = db.execute(
                        "SELECT count(*) FROM grants WHERE user=?", (user,)
                    ).fetchone()[0]
                    total = db.execute("SELECT count(*) FROM grants").fetchone()[0]
                    if previous is None and (
                        count >= self.config["maxAddresses"] or total >= 4096
                    ):
                        raise ValueError(
                            "Active address limit reached; wait for an old grant to expire"
                        )
                    expiry = now + self.config["leaseSeconds"]
                    db.execute(
                        "INSERT INTO grants VALUES (?, ?, ?) ON CONFLICT(user, ip) "
                        "DO UPDATE SET expiry=excluded.expiry",
                        (user, address, expiry),
                    )
                if user is None or address is not None:
                    rows = db.execute(
                        "SELECT ip, max(expiry) FROM grants GROUP BY ip"
                    ).fetchall()
                    self.apply(rows, now)
                if user is None:
                    return []
                return [
                    {"ip": ip, "expires": expiry}
                    for ip, expiry in db.execute(
                        "SELECT ip, expiry FROM grants WHERE user=? ORDER BY ip",
                        (user,),
                    )
                ]

    def apply(self, rows, now):
        sets = {4: [], 6: []}
        for address, expiry in rows:
            if "%" in address:
                raise ValueError("Scoped addresses are not supported")
            ip = ipaddress.ip_address(address)
            remaining = min(self.config["leaseSeconds"], math.floor(expiry - now))
            if remaining > 0:
                sets[ip.version].append(f"{ip} timeout {remaining}s")
        lines = ["destroy table inet minecraft_access", "table inet minecraft_access {"]
        for family, kind in ((4, "ipv4_addr"), (6, "ipv6_addr")):
            lines.append(f"set players{family} {{ type {kind}; flags timeout;")
            if sets[family]:
                lines.append("elements = { " + ", ".join(sets[family]) + " }")
            lines.append("}")
        lines += ["chain input { type filter hook input priority -10; policy accept;"]
        for protocol in ("tcp", "udp"):
            ports = self.config["ports"][protocol]
            if ports:
                if any(
                    type(port) is not int or not 1 <= port <= 65535 for port in ports
                ):
                    raise ValueError("Invalid protected port")
                lines.append(
                    protocol + " dport { " + ", ".join(map(str, ports)) + " } jump gate"
                )
        lines += [
            "}",
            "chain gate {",
            "ct state established return",
            "ip saddr @players4 return",
            "ip6 saddr @players6 return",
            "drop",
            "}",
            "}",
        ]
        subprocess.run(
            [self.nft, "-f", "-"],
            input="\n".join(lines),
            text=True,
            check=True,
            capture_output=True,
            timeout=10,
        )


def create_app(grants):
    async def handle(request):
        try:
            data = await request.json()
            if (
                not isinstance(data, dict)
                or set(data) - {"user", "ip"}
                or not isinstance(data.get("user"), str)
            ):
                raise ValueError("Invalid request")
            if request.path == "/grant" and not isinstance(data.get("ip"), str):
                raise ValueError("Missing address")
            result = await asyncio.to_thread(
                grants.run,
                data["user"],
                data.get("ip") if request.path == "/grant" else None,
            )
            return web.json_response(result)
        except (ValueError, TypeError):
            return web.json_response(
                {"error": "Invalid request or active address limit reached"}, status=400
            )
        except (OSError, sqlite3.Error, subprocess.SubprocessError):
            logger.error("Grant reconciliation failed")
            return web.json_response(
                {"error": "Firewall update unavailable"}, status=503
            )

    app = web.Application(client_max_size=1024)
    app.router.add_post("/grant", handle)
    app.router.add_post("/list", handle)
    return app


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("config")
    parser.add_argument("--restore", action="store_true")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text())
    os.umask(0o007)
    grants = Grants(config, "/var/lib/minecraft-access-firewall", "@nft@")
    if args.restore:
        # Install a closed guard before touching persistent state on first boot.
        # A corrupt database must not leave a new host exposed.
        existing = subprocess.run(
            [grants.nft, "list", "table", "inet", "minecraft_access"],
            capture_output=True,
            timeout=10,
            check=False,
        )
        if existing.returncode:
            grants.apply([], time.time())
        grants.run()
    else:
        web.run_app(
            create_app(grants),
            path="/run/minecraft-access-firewall/http.sock",
            access_log=None,
            print=None,
        )


if __name__ == "__main__":
    main()
