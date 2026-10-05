"""Private grant broker and atomic nftables reconciliation for named port groups."""

import argparse
import asyncio
import fcntl
import fnmatch
import ipaddress
import json
import logging
import math
import os
import re
import secrets
import sqlite3
import subprocess
import time
from contextlib import closing
from pathlib import Path

from aiohttp import web

logger = logging.getLogger(__name__)


class GrantError(ValueError):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


class Grants:
    def __init__(self, config, directory, nft):
        self.config = config
        self.directory = Path(directory)
        self.nft = nft
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)

    def prepare(self, db, now):
        """Create the table, drop expired and policy-incompatible grants; whether any were pruned."""
        db.execute(
            "CREATE TABLE IF NOT EXISTS grants (user TEXT, ip TEXT, group_id TEXT, expiry REAL, "
            "PRIMARY KEY(user, ip, group_id))"
        )
        db.execute("CREATE INDEX IF NOT EXISTS grants_expiry ON grants(expiry)")
        db.execute("DELETE FROM grants WHERE expiry <= ?", (now,))
        configured = self.config["groups"]
        reconciled = False
        for group, ip in db.execute("SELECT DISTINCT group_id, ip FROM grants").fetchall():
            if group not in configured or ipaddress.ip_address(
                ip
            ).version not in configured[group].get("addressFamilies", [4, 6]):
                db.execute("DELETE FROM grants WHERE group_id=? AND ip=?", (group, ip))
                reconciled = True
        return reconciled

    def renew(self, address, group, now=None):
        """Extend every live grant of `address` in `group` by a full lease; never create one.
        Returns how many grants were extended."""
        if group not in self.config["groups"]:
            raise GrantError("invalid_groups")
        if "%" in address:
            raise ValueError("Scoped addresses are not supported")
        address = str(ipaddress.ip_address(address))
        with (self.directory / "lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            now = time.time() if now is None else now
            with closing(sqlite3.connect(self.directory / "grants.sqlite")) as db, db:
                reconciled = self.prepare(db, now)
                renewed = db.execute(
                    "UPDATE grants SET expiry=? WHERE ip=? AND group_id=?",
                    (now + self.config["leaseSeconds"], address, group),
                ).rowcount
                if renewed or reconciled:
                    rows = db.execute(
                        "SELECT group_id, ip, max(expiry) FROM grants GROUP BY group_id, ip"
                    ).fetchall()
                    self.apply(rows, now)
                return renewed

    def run(self, user=None, address=None, now=None, groups=None):
        with (self.directory / "lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            now = time.time() if now is None else now
            with closing(sqlite3.connect(self.directory / "grants.sqlite")) as db, db:
                configured = self.config["groups"]
                reconciled = self.prepare(db, now)
                if user is not None and (
                    not isinstance(user, str)
                    or not user.isascii()
                    or not user.isdigit()
                    or len(user) > 20
                ):
                    raise ValueError("Invalid account")
                if address is not None:
                    if (
                        not isinstance(groups, list)
                        or not groups
                        or any(
                            not isinstance(group, str) or group not in configured
                            for group in groups
                        )
                    ):
                        raise GrantError("invalid_groups")
                    groups = sorted(set(groups))
                    if "%" in address:
                        raise ValueError("Scoped addresses are not supported")
                    address = str(ipaddress.ip_address(address))
                    if any(
                        ipaddress.ip_address(address).version
                        not in configured[g].get("addressFamilies", [4, 6])
                        for g in groups
                    ):
                        raise GrantError("unsupported_family")
                    previous = db.execute(
                        "SELECT 1 FROM grants WHERE user=? AND ip=?", (user, address)
                    ).fetchone()
                    count = db.execute(
                        "SELECT count(DISTINCT ip) FROM grants WHERE user=?", (user,)
                    ).fetchone()[0]
                    if previous is None and count >= self.config["maxAddresses"]:
                        raise GrantError("address_limit")
                    for group in groups:
                        db.execute(
                            "INSERT INTO grants VALUES (?, ?, ?, ?) ON CONFLICT(user, ip, group_id) "
                            "DO UPDATE SET expiry=excluded.expiry",
                            (user, address, group, now + self.config["leaseSeconds"]),
                        )
                    if db.execute("SELECT count(*) FROM grants").fetchone()[0] > 4096:
                        raise GrantError("grant_limit")
                if user is None or address is not None or reconciled:
                    rows = db.execute(
                        "SELECT group_id, ip, max(expiry) FROM grants GROUP BY group_id, ip"
                    ).fetchall()
                    self.apply(rows, now)
                if user is None:
                    return []
                return [
                    {"ip": ip, "group": group, "expires": expiry}
                    for ip, group, expiry in db.execute(
                        "SELECT ip, group_id, expiry FROM grants WHERE user=? ORDER BY group_id, ip",
                        (user,),
                    )
                ]

    def apply(self, rows, now):
        groups = self.config["groups"]
        sets = {group: {4: [], 6: []} for group in groups}
        for group, address, expiry in rows:
            if group not in sets:
                continue
            if "%" in address:
                raise ValueError("Scoped addresses are not supported")
            ip = ipaddress.ip_address(address)
            if ip.version not in groups[group].get("addressFamilies", [4, 6]):
                continue
            remaining = min(self.config["leaseSeconds"], math.floor(expiry - now))
            if remaining > 0:
                sets[group][ip.version].append(f"{ip} timeout {remaining}s")
        # Retire the old guard even when its grants are not being imported.
        # Otherwise it can keep blocking ports allowed by Punch until reboot.
        lines = ["destroy table inet punch", "destroy table inet minecraft_access"]
        lines.append("table inet punch {")
        for index, group in enumerate(groups):
            for family, kind in ((4, "ipv4_addr"), (6, "ipv6_addr")):
                lines.append(f"set g{index}_v{family} {{ type {kind}; flags timeout;")
                if sets[group][family]:
                    lines.append(
                        "elements = { " + ", ".join(sets[group][family]) + " }"
                    )
                lines.append("}")
        lines.append(
            "chain input { type filter hook input priority -10; policy accept;"
        )
        for protocol in ("tcp", "udp"):
            all_ports = set()
            for index, group in enumerate(groups):
                ports = groups[group]["ports"][protocol]
                if not ports:
                    continue
                if any(
                    type(port) is not int or not 1 <= port <= 65535 for port in ports
                ):
                    raise ValueError("Invalid protected port")
                all_ports.update(ports)
                match = (
                    protocol
                    + " dport { "
                    + ", ".join(map(str, sorted(set(ports))))
                    + " }"
                )
                for family, expression in ((4, "ip"), (6, "ip6")):
                    lines.append(
                        f"{match} {expression} saddr @g{index}_v{family} return"
                    )
            if all_ports:
                match = (
                    protocol
                    + " dport { "
                    + ", ".join(map(str, sorted(all_ports)))
                    + " }"
                )
                lines.append(match + " ct state established return")
                # All matching groups were considered before denying: overlapping ports are a union.
                lines.append(match + " drop")
        lines += ["}", "}"]
        subprocess.run(
            [self.nft, "-f", "-"],
            input="\n".join(lines),
            text=True,
            check=True,
            capture_output=True,
            timeout=10,
        )


# A player's login as the server logs it, anchored on the logger so that chat
# quoting such a line can't match. Java prints IPv6 without brackets.
LOGIN = re.compile(
    r"\[\d\d:\d\d:\d\d\] \[Server thread/INFO\] \[minecraft/PlayerList\]: "
    r"[A-Za-z0-9_]{1,16}\[/([0-9A-Fa-f:.]+):\d{1,5}\] logged in with entity id \d+ at \("
)
RENEW_INTERVAL = 3600


def login_address(line):
    """The address a Minecraft login line came from, or None."""
    match = LOGIN.match(line)
    if match is None:
        return None
    try:
        ip = ipaddress.ip_address(match.group(1))
    except ValueError:
        return None
    if ip.version == 6 and ip.ipv4_mapped is not None:
        ip = ip.ipv4_mapped
    return str(ip)


async def watch_logins(grants, group, units, journalctl="@journalctl@"):
    """Renew a group's grants for addresses that log in to its Minecraft servers.
    Only an address that already has a grant can connect, so this never creates one."""
    # Not --unit: it expands globs once, against units already in the journal.
    command = [
        journalctl, "--follow", "--lines=0", "--output=json",
        "--output-fields=MESSAGE,_SYSTEMD_UNIT", "--grep=logged in with entity id",
    ]
    renewed = {}
    while True:
        process = await asyncio.create_subprocess_exec(
            *command, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )
        try:
            async for raw in process.stdout:
                try:
                    entry = json.loads(raw)
                except ValueError:
                    continue
                unit, message = entry.get("_SYSTEMD_UNIT"), entry.get("MESSAGE")
                if not isinstance(unit, str) or not isinstance(message, str):
                    continue
                if not any(fnmatch.fnmatchcase(unit, pattern) for pattern in units):
                    continue
                address = login_address(message)
                if address is None:
                    continue
                now = time.monotonic()
                if now - renewed.get(address, -RENEW_INTERVAL) < RENEW_INTERVAL:
                    continue
                renewed[address] = now
                try:
                    count = await asyncio.to_thread(grants.renew, address, group)
                except (OSError, ValueError, sqlite3.Error, subprocess.SubprocessError):
                    logger.error("Renewing a %s grant after a login failed", group)
                    continue
                if count:
                    logger.info("A %s login renewed %d grant(s)", group, count)
        finally:
            if process.returncode is None:
                process.kill()
            await process.wait()
        error = (await process.stderr.read()).decode("utf-8", "replace").strip()[-500:]
        logger.error("journalctl for %s logins exited (%s); restarting in 10 s", group, error)
        await asyncio.sleep(10)


def create_app(grants, token=None):
    async def handle(request):
        if token is not None and not secrets.compare_digest(
            request.headers.get("Authorization", "").encode(),
            ("Bearer " + token).encode(),
        ):
            raise web.HTTPUnauthorized()
        try:
            data = await request.json()
            if (
                not isinstance(data, dict)
                or set(data) - {"user", "ip", "groups"}
                or not isinstance(data.get("user"), str)
            ):
                raise ValueError("Invalid request")
            if request.path == "/grant" and not isinstance(data.get("ip"), str):
                raise ValueError("Missing address")
            result = await asyncio.to_thread(
                grants.run,
                data["user"],
                data.get("ip") if request.path == "/grant" else None,
                groups=data.get("groups"),
            )
            return web.json_response(result)
        except GrantError as error:
            return web.json_response({"error": error.code}, status=400)
        except (ValueError, TypeError):
            return web.json_response({"error": "invalid_request"}, status=400)
        except (OSError, sqlite3.Error, subprocess.SubprocessError):
            logger.error("Grant reconciliation failed")
            return web.json_response({"error": "firewall_unavailable"}, status=503)

    app = web.Application(client_max_size=4096)
    app.router.add_post("/grant", handle)
    app.router.add_post("/list", handle)
    return app


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("config")
    parser.add_argument("--restore", action="store_true")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text())
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    os.umask(0o007)
    grants = Grants(
        config,
        "/var/lib/punch-firewall",
        "@nft@",
    )
    if args.restore:
        existing = subprocess.run(
            [grants.nft, "list", "table", "inet", "punch"],
            capture_output=True,
            timeout=10,
            check=False,
        )
        if existing.returncode:
            grants.apply([], time.time())
        grants.run()
    else:
        address = config.get("listenAddress")
        token = None
        if address is not None:
            token = (
                (Path(os.environ["CREDENTIALS_DIRECTORY"]) / "broker-token")
                .read_text()
                .strip()
            )
            if len(token) < 32:
                raise ValueError("Broker token must contain at least 32 characters")
        endpoint = (
            {"host": address, "port": config.get("listenPort", 9781)}
            if address is not None
            else {"path": "/run/punch-firewall/http.sock"}
        )
        app = create_app(grants, token)

        async def watchers(app):
            tasks = [
                asyncio.create_task(watch_logins(grants, name, group["minecraftLoginUnits"]))
                for name, group in config["groups"].items()
                if group.get("minecraftLoginUnits")
            ]
            yield
            for task in tasks:
                task.cancel()

        app.cleanup_ctx.append(watchers)
        web.run_app(
            app,
            **endpoint,
            access_log=None,
            print=None,
        )


if __name__ == "__main__":
    main()
