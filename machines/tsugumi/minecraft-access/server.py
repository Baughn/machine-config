"""Discord login, remembered sessions, and dual-stack address discovery."""

import argparse
import hashlib
import ipaddress
import json
import logging
import os
import secrets
import sqlite3
import time
from pathlib import Path
from urllib.parse import urlencode

import aiohttp
from aiohttp import web

COOKIE = "__Host-minecraft-session"
LOGIN_COOKIE = "__Host-minecraft-login"
logger = logging.getLogger(__name__)


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


class Store:
    def __init__(self, path):
        self.db = sqlite3.connect(path)
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY, user TEXT, csrf TEXT, expiry REAL);
            CREATE TABLE IF NOT EXISTS logins (
                state TEXT PRIMARY KEY, browser TEXT, expiry REAL);
            CREATE TABLE IF NOT EXISTS tickets (
                id TEXT PRIMARY KEY, session TEXT, family INTEGER, expiry REAL);
            CREATE TABLE IF NOT EXISTS rates (key TEXT PRIMARY KEY, expiry REAL);
        """)

    def clean(self, now):
        with self.db:
            for table in ("sessions", "logins", "tickets", "rates"):
                self.db.execute(f"DELETE FROM {table} WHERE expiry <= ?", (now,))

    def rate(self, key, seconds, now):
        self.clean(now)
        with self.db:
            if self.db.execute("SELECT 1 FROM rates WHERE key=?", (key,)).fetchone():
                raise web.HTTPTooManyRequests(text="Please wait before trying again.")
            if self.db.execute("SELECT count(*) FROM rates").fetchone()[0] >= 8192:
                raise web.HTTPServiceUnavailable(text="Please try again later.")
            self.db.execute("INSERT INTO rates VALUES (?, ?)", (key, now + seconds))

    def session(self, token, now):
        return self.db.execute(
            "SELECT user, csrf, expiry FROM sessions WHERE id=? AND expiry>?",
            (digest(token), now),
        ).fetchone()


class Frontend:
    def __init__(self, config, store, secret, http, broker, assets):
        self.config = config
        self.store = store
        self.secret = secret
        self.http = http
        self.broker = broker
        self.assets = Path(assets)
        self.origin = "https://" + config["hostname"]
        self.hosts = {4: config["ipv4Hostname"], 6: config["ipv6Hostname"]}

    def source(self, request):
        # Only Caddy can connect to the service's Unix socket. Caddy overwrites
        # this header from its TCP peer, independently of X-Forwarded-For.
        try:
            value = request.headers.get("X-Minecraft-Client-IP", "")
            if "%" in value:
                raise ValueError("Scoped address")
            return ipaddress.ip_address(value)
        except ValueError:
            raise web.HTTPBadRequest(
                text="Unable to determine your network address."
            ) from None

    def main_host(self, request):
        if request.host != self.config["hostname"]:
            raise web.HTTPNotFound()

    def session(self, request):
        self.main_host(request)
        session = self.store.session(request.cookies.get(COOKIE, ""), time.time())
        if session is None:
            raise web.HTTPUnauthorized(text="Please log in with Discord.")
        return session

    def csrf(self, request, session):
        if request.headers.get("Origin") != self.origin or not secrets.compare_digest(
            request.headers.get("X-CSRF-Token", ""), session[1]
        ):
            raise web.HTTPForbidden(text="Invalid page session; reload this page.")

    async def page(self, request):
        self.main_host(request)
        return web.Response(
            text=(self.assets / "index.html").read_text(), content_type="text/html"
        )

    async def script(self, request):
        self.main_host(request)
        return web.Response(
            text=(self.assets / "app.js").read_text(),
            content_type="application/javascript",
        )

    async def login(self, request):
        self.main_host(request)
        now = time.time()
        self.store.rate("login:" + digest(str(self.source(request))), 10, now)
        state, browser = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
        with self.store.db:
            if (
                self.store.db.execute("SELECT count(*) FROM logins").fetchone()[0]
                >= 4096
            ):
                raise web.HTTPServiceUnavailable(text="Please try logging in later.")
            self.store.db.execute(
                "INSERT INTO logins VALUES (?, ?, ?)",
                (digest(state), digest(browser), now + 600),
            )
        query = urlencode(
            {
                "client_id": self.config["clientId"],
                "response_type": "code",
                "redirect_uri": self.origin + "/oauth/callback",
                "scope": "identify guilds.members.read",
                "state": state,
            }
        )
        response = web.HTTPFound(
            self.config.get("authorizeUrl", "https://discord.com/oauth2/authorize")
            + "?"
            + query
        )
        response.set_cookie(
            LOGIN_COOKIE,
            browser,
            max_age=600,
            secure=True,
            httponly=True,
            samesite="Lax",
            path="/",
        )
        return response

    async def callback(self, request):
        self.main_host(request)
        now = time.time()
        with self.store.db:
            login = self.store.db.execute(
                "DELETE FROM logins WHERE state=? RETURNING browser, expiry",
                (digest(request.query.get("state", "")),),
            ).fetchone()
        if (
            login is None
            or login[1] <= now
            or not secrets.compare_digest(
                login[0], digest(request.cookies.get(LOGIN_COOKIE, ""))
            )
        ):
            raise web.HTTPForbidden(
                text="Login expired or invalid. Please start again."
            )
        if "code" not in request.query or "error" in request.query:
            raise web.HTTPForbidden(text="Discord login was not completed.")
        base = self.config.get("discordApi", "https://discord.com/api/v10")
        try:
            async with self.http.post(
                base + "/oauth2/token",
                data={
                    "client_id": self.config["clientId"],
                    "client_secret": self.secret,
                    "grant_type": "authorization_code",
                    "code": request.query["code"],
                    "redirect_uri": self.origin + "/oauth/callback",
                },
            ) as response:
                if response.status != 200:
                    raise web.HTTPBadGateway(
                        text="Discord login failed. Please start again."
                    )
                token = (await response.json())["access_token"]
            async with self.http.get(
                base + "/users/@me/guilds/" + self.config["guildId"] + "/member",
                headers={"Authorization": "Bearer " + token},
            ) as response:
                if response.status in (403, 404):
                    raise web.HTTPForbidden(
                        text="Join the Discord server and obtain the Minecraft players role first."
                    )
                if response.status != 200:
                    raise web.HTTPBadGateway(
                        text="Discord is unavailable. Please try again later."
                    )
                member = await response.json()
            if self.config["roleId"] not in member.get("roles", []):
                raise web.HTTPForbidden(
                    text="You need the Minecraft players role to enable access."
                )
            user = member["user"]["id"]
            if (
                not isinstance(user, str)
                or not user.isascii()
                or not user.isdigit()
                or len(user) > 20
            ):
                raise ValueError("Invalid Discord user")
        except (aiohttp.ClientError, TimeoutError, KeyError, ValueError, TypeError):
            raise web.HTTPBadGateway(
                text="Discord is unavailable. Please try again later."
            ) from None
        # Tokens, including any returned refresh token, never enter persistent storage.
        token = secrets.token_urlsafe(32)
        csrf = secrets.token_urlsafe(32)
        duration = self.config["sessionSeconds"]
        self.store.clean(now)
        with self.store.db:
            if (
                self.store.db.execute("SELECT count(*) FROM sessions").fetchone()[0]
                >= 4096
            ):
                raise web.HTTPServiceUnavailable(text="Please try logging in later.")
            # Bound remembered browsers per account, retaining the newest eight.
            self.store.db.execute(
                "DELETE FROM sessions WHERE id IN (SELECT id FROM sessions "
                "WHERE user=? ORDER BY expiry DESC LIMIT -1 OFFSET 7)",
                (user,),
            )
            self.store.db.execute(
                "INSERT INTO sessions VALUES (?, ?, ?, ?)",
                (digest(token), user, csrf, now + duration),
            )
        result = web.HTTPFound("/")
        result.set_cookie(
            COOKIE,
            token,
            max_age=duration,
            secure=True,
            httponly=True,
            samesite="Lax",
            path="/",
        )
        result.del_cookie(
            LOGIN_COOKIE, path="/", secure=True, httponly=True, samesite="Lax"
        )
        return result

    async def broker_call(self, path, data):
        try:
            async with self.broker.post(
                "http://localhost/" + path, json=data
            ) as response:
                if response.status == 400:
                    raise web.HTTPBadRequest(
                        text="Active address limit reached. Wait for an old address to expire."
                    )
                if response.status != 200:
                    raise web.HTTPServiceUnavailable(
                        text="Access could not be updated. Please try again."
                    )
                return await response.json()
        except (aiohttp.ClientError, TimeoutError):
            raise web.HTTPServiceUnavailable(
                text="Access service unavailable. Please try again."
            ) from None

    async def status(self, request):
        user, csrf, expiry = self.session(request)
        grants = await self.broker_call("list", {"user": user})
        return web.json_response(
            {"csrf": csrf, "sessionExpires": expiry, "grants": grants}
        )

    async def visit(self, request):
        session = self.session(request)
        self.csrf(request, session)
        now = time.time()
        self.store.rate("visit:" + session[0], 30, now)
        session_id = digest(request.cookies[COOKIE])
        tickets = []
        with self.store.db:
            self.store.db.execute("DELETE FROM tickets WHERE session=?", (session_id,))
            for family, host in self.hosts.items():
                token = secrets.token_urlsafe(32)
                self.store.db.execute(
                    "INSERT INTO tickets VALUES (?, ?, ?, ?)",
                    (digest(token), session_id, family, now + 60),
                )
                tickets.append(
                    {
                        "family": family,
                        "url": "https://" + host + "/minecraft-access/authorize",
                        "ticket": token,
                    }
                )
        return web.json_response(tickets)

    async def authorize(self, request):
        if (
            request.headers.get("Origin") != self.origin
            or request.host not in self.hosts.values()
        ):
            raise web.HTTPForbidden()
        if request.method == "OPTIONS":
            return web.Response(
                status=204,
                headers={
                    "Access-Control-Allow-Methods": "POST",
                    "Access-Control-Allow-Headers": "Content-Type",
                },
            )
        try:
            data = await request.json()
            ticket_id = digest(data["ticket"])
        except (ValueError, KeyError, TypeError, AttributeError):
            raise web.HTTPBadRequest(text="Invalid authorization ticket.") from None
        address = self.source(request)
        with self.store.db:
            ticket = self.store.db.execute(
                "DELETE FROM tickets WHERE id=? RETURNING session, family, expiry",
                (ticket_id,),
            ).fetchone()
        now = time.time()
        if (
            ticket is None
            or ticket[2] <= now
            or self.hosts[ticket[1]] != request.host
            or address.version != ticket[1]
        ):
            raise web.HTTPForbidden(
                text="Authorization ticket expired or used on the wrong network."
            )
        session = self.store.db.execute(
            "SELECT user FROM sessions WHERE id=? AND expiry>?", (ticket[0], now)
        ).fetchone()
        if session is None:
            raise web.HTTPUnauthorized(text="Please log in again.")
        grants = await self.broker_call(
            "grant", {"user": session[0], "ip": str(address)}
        )
        return web.json_response({"ip": str(address), "grants": grants})

    async def logout(self, request):
        session = self.session(request)
        self.csrf(request, session)
        with self.store.db:
            self.store.db.execute(
                "DELETE FROM sessions WHERE id=?", (digest(request.cookies[COOKIE]),)
            )
        response = web.Response(text="Logged out.")
        response.del_cookie(
            COOKIE, path="/", secure=True, httponly=True, samesite="Lax"
        )
        return response


def create_app(frontend):
    @web.middleware
    async def security(request, handler):
        try:
            response = await handler(request)
        except web.HTTPException as error:
            response = error
        except Exception as error:  # noqa: BLE001 -- prevent framework logging of credentials
            # Do not include request URLs, tokens, or client IPs in logs.
            logger.error("Frontend request failed (%s)", type(error).__name__)
            response = web.Response(
                status=500, text="Access service unavailable. Please try again."
            )
        response.headers.update(
            {
                "Cache-Control": "no-store",
                "Referrer-Policy": "no-referrer",
                "X-Content-Type-Options": "nosniff",
                "Content-Security-Policy": "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; "
                "connect-src 'self' "
                + " ".join("https://" + host for host in frontend.hosts.values())
                + "; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
            }
        )
        if (
            request.path == "/minecraft-access/authorize"
            and request.headers.get("Origin") == frontend.origin
        ):
            response.headers["Access-Control-Allow-Origin"] = frontend.origin
            response.headers["Vary"] = "Origin"
        return response

    app = web.Application(client_max_size=2048, middlewares=[security])
    app.add_routes(
        [
            web.get("/", frontend.page),
            web.get("/app.js", frontend.script),
            web.get("/login", frontend.login),
            web.get("/oauth/callback", frontend.callback),
            web.get("/api/status", frontend.status),
            web.post("/api/visit", frontend.visit),
            web.post("/api/logout", frontend.logout),
            web.post("/minecraft-access/authorize", frontend.authorize),
            web.options("/minecraft-access/authorize", frontend.authorize),
        ]
    )
    return app


async def application(config):
    secret = (
        (Path(os.environ["CREDENTIALS_DIRECTORY"]) / "discord-secret")
        .read_text()
        .strip()
    )
    store = Store(Path(os.environ["STATE_DIRECTORY"]) / "sessions.sqlite")
    http = aiohttp.ClientSession(
        timeout=aiohttp.ClientTimeout(total=15), raise_for_status=False
    )
    broker = aiohttp.ClientSession(
        connector=aiohttp.UnixConnector(
            path="/run/minecraft-access-firewall/http.sock"
        ),
        timeout=aiohttp.ClientTimeout(total=15),
    )
    app = create_app(
        Frontend(config, store, secret, http, broker, Path(__file__).parent)
    )

    async def cleanup(app):
        await http.close()
        await broker.close()
        store.db.close()

    app.on_cleanup.append(cleanup)
    return app


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("config")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text())
    os.umask(0o007)
    web.run_app(
        application(config),
        path="/run/minecraft-access/http.sock",
        access_log=None,
        print=None,
    )


if __name__ == "__main__":
    main()
