import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.parse import parse_qs, urlparse

import aiohttp
from aiohttp import web
from aiohttp.test_utils import TestClient, TestServer

SOURCE = Path(__file__).resolve().parents[1] / "machines/tsugumi/minecraft-access"


def module(name):
    spec = importlib.util.spec_from_file_location(name, SOURCE / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    sys.modules[name] = result
    spec.loader.exec_module(result)
    return result


server = module("server")
firewall = module("firewall")
CONFIG = {
    "hostname": "minecraft.brage.info",
    "ipv4Hostname": "v4.brage.info",
    "ipv6Hostname": "v6.brage.info",
    "clientId": "1",
    "guildId": "2",
    "roleId": "3",
    "sessionSeconds": 90 * 86400,
    "leaseSeconds": 14 * 86400,
    "maxAddresses": 16,
    "ports": {"tcp": [25565, 25566], "udp": [24454]},
}


class GrantTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.grants = firewall.Grants(CONFIG, self.temp.name, "/nft")
        self.nft = patch.object(subprocess, "run").start()
        self.addCleanup(patch.stopall)

    def test_renewal_shared_ip_and_restore_remaining_lifetime(self):
        self.grants.run("1", "192.0.2.1", now=100)
        self.grants.run("2", "192.0.2.1", now=200)
        self.grants.run("1", "2001:db8::1", now=300)
        self.grants.run(now=400)
        rules = self.nft.call_args.kwargs["input"]
        self.assertIn("192.0.2.1 timeout 1209400s", rules)
        self.assertIn("2001:db8::1 timeout 1209500s", rules)
        self.assertIn("tcp dport { 25565, 25566 }", rules)
        self.assertIn("udp dport { 24454 }", rules)
        self.assertEqual(len(self.grants.run("1", now=400)), 2)
        self.grants.run(now=300 + CONFIG["leaseSeconds"])
        self.assertNotIn("elements", self.nft.call_args.kwargs["input"])

    def test_limit_and_no_extension_on_failed_update(self):
        self.grants.config = CONFIG | {"maxAddresses": 1}
        self.grants.run("1", "192.0.2.1", now=100)
        with self.assertRaises(ValueError):
            self.grants.run("1", "192.0.2.2", now=200)
        self.nft.side_effect = subprocess.CalledProcessError(1, "nft")
        with self.assertRaises(subprocess.CalledProcessError):
            self.grants.run("1", "192.0.2.1", now=200)
        self.assertEqual(
            self.grants.run("1", now=300)[0]["expires"], 100 + CONFIG["leaseSeconds"]
        )

    def test_rejects_networks_commands_and_invalid_accounts(self):
        for address in (
            "192.0.2.0/24",
            "1.1.1.1; flush ruleset",
            "not-an-ip",
            "::1%; flush ruleset",
        ):
            with self.assertRaises(ValueError):
                self.grants.run("1", address)
        for user in ("", "not-an-id", "1" * 21):
            with self.assertRaises(ValueError):
                self.grants.run(user, "192.0.2.1")
        self.nft.assert_not_called()


class FrontendTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = server.Store(Path(self.temp.name) / "sessions.sqlite")
        self.addCleanup(self.store.db.close)
        self.roles = ["3"]
        self.discord_status = 200
        self.token_calls = 0

        async def token(request):
            self.token_calls += 1
            data = await request.post()
            self.assertEqual(data["client_secret"], "test-secret")
            self.assertEqual(
                data["redirect_uri"], "https://minecraft.brage.info/oauth/callback"
            )
            return web.json_response(
                {
                    "access_token": "sensitive-access",
                    "refresh_token": "sensitive-refresh",
                }
            )

        async def member(request):
            self.assertEqual(
                request.headers["Authorization"], "Bearer sensitive-access"
            )
            return web.json_response(
                {"roles": self.roles, "user": {"id": "42"}}, status=self.discord_status
            )

        discord_app = web.Application()
        discord_app.router.add_post("/oauth2/token", token)
        discord_app.router.add_get("/users/@me/guilds/2/member", member)
        self.discord = TestServer(discord_app)
        await self.discord.start_server()
        self.addAsyncCleanup(self.discord.close)
        self.http = aiohttp.ClientSession()
        self.addAsyncCleanup(self.http.close)
        self.grants = firewall.Grants(CONFIG, Path(self.temp.name) / "grants", "/nft")
        self.broker_server = TestServer(firewall.create_app(self.grants))
        await self.broker_server.start_server()
        self.addAsyncCleanup(self.broker_server.close)
        self.nft = patch.object(subprocess, "run").start()
        self.addCleanup(patch.stopall)

        class Broker:
            def post(inner, url, **kwargs):
                return self.http.post(
                    self.broker_server.make_url(urlparse(url).path), **kwargs
                )

        frontend = server.Frontend(
            CONFIG | {"discordApi": str(self.discord.make_url(""))},
            self.store,
            "test-secret",
            self.http,
            Broker(),
            SOURCE,
        )
        self.client = TestClient(
            TestServer(server.create_app(frontend)), cookie_jar=aiohttp.DummyCookieJar()
        )
        await self.client.start_server()
        self.addAsyncCleanup(self.client.close)
        self.headers = {
            "Host": CONFIG["hostname"],
            "X-Minecraft-Client-IP": "192.0.2.1",
        }

    async def login(self):
        response = await self.client.get(
            "/login", headers=self.headers, allow_redirects=False
        )
        self.assertEqual(response.status, 302)
        state = parse_qs(urlparse(response.headers["Location"]).query)["state"][0]
        cookie = response.cookies[server.LOGIN_COOKIE].value
        callback = "/oauth/callback?" + server.urlencode(
            {"state": state, "code": "test-code"}
        )
        response = await self.client.get(
            callback,
            headers=self.headers | {"Cookie": server.LOGIN_COOKIE + "=" + cookie},
            allow_redirects=False,
        )
        if response.status == 302:
            session = response.cookies[server.COOKIE]
            self.assertTrue(session["secure"] and session["httponly"])
            self.assertEqual(int(session["max-age"]), 90 * 86400)
            self.headers["Cookie"] = server.COOKIE + "=" + session.value
        return response, callback

    async def tickets(self):
        status = await (
            await self.client.get("/api/status", headers=self.headers)
        ).json()
        response = await self.client.post(
            "/api/visit",
            headers=self.headers
            | {
                "Origin": "https://" + CONFIG["hostname"],
                "X-CSRF-Token": status["csrf"],
            },
        )
        return response, status

    async def authorize(self, ticket, address, **extra):
        return await self.client.post(
            "/minecraft-access/authorize",
            json={"ticket": ticket["ticket"]},
            headers={
                "Host": urlparse(ticket["url"]).hostname,
                "Origin": "https://" + CONFIG["hostname"],
                "X-Minecraft-Client-IP": address,
                **extra,
            },
        )

    async def test_login_dual_stack_and_renew_without_discord(self):
        response, callback = await self.login()
        self.assertEqual(response.status, 302)
        replay = await self.client.get(
            callback, headers=self.headers, allow_redirects=False
        )
        self.assertEqual(replay.status, 403)
        response, _ = await self.tickets()
        self.assertEqual(response.status, 200)
        tickets = await response.json()
        for ticket, ip in zip(tickets, ("192.0.2.1", "2001:db8::1")):
            response = await self.authorize(
                ticket, ip, **{"X-Forwarded-For": "203.0.113.9"}
            )
            self.assertEqual(response.status, 200, await response.text())
            self.assertEqual((await response.json())["ip"], ip)
            self.assertEqual((await self.authorize(ticket, ip)).status, 403)
        self.assertEqual(len(self.grants.run("42")), 2)
        self.assertEqual((await self.tickets())[0].status, 429)
        with self.store.db:
            self.store.db.execute("DELETE FROM rates")
        self.roles = []
        self.discord_status = 503
        self.assertEqual((await self.tickets())[0].status, 200)
        self.assertEqual(self.token_calls, 1)
        raw = (Path(self.temp.name) / "sessions.sqlite").read_bytes()
        self.assertNotIn(b"sensitive-access", raw)
        self.assertNotIn(b"sensitive-refresh", raw)

    async def test_role_failure_and_discord_outage(self):
        self.roles = []
        response, _ = await self.login()
        self.assertEqual(response.status, 403)
        self.assertEqual(
            self.store.db.execute("SELECT count(*) FROM sessions").fetchone()[0], 0
        )
        self.store.db.execute("DELETE FROM rates")
        self.roles = ["3"]
        self.discord_status = 503
        response, _ = await self.login()
        self.assertEqual(response.status, 502)
        self.nft.assert_not_called()

    async def test_csrf_origin_family_and_expired_session(self):
        await self.login()
        self.assertEqual(
            (await self.client.post("/api/visit", headers=self.headers)).status, 403
        )
        response, _ = await self.tickets()
        tickets = await response.json()
        response = await self.authorize(
            tickets[0], "192.0.2.1", Origin="https://evil.example"
        )
        self.assertEqual(response.status, 403)
        self.assertNotIn("Access-Control-Allow-Origin", response.headers)
        self.assertEqual((await self.authorize(tickets[0], "2001:db8::1")).status, 403)
        self.store.db.execute("UPDATE sessions SET expiry=0")
        self.store.db.commit()
        self.assertEqual((await self.authorize(tickets[1], "2001:db8::1")).status, 401)
        self.assertEqual(
            (await self.client.get("/api/status", headers=self.headers)).status, 401
        )
        self.nft.assert_not_called()

    async def test_logout_invalidates_tickets_but_keeps_grants(self):
        await self.login()
        response, status = await self.tickets()
        tickets = await response.json()
        self.assertEqual((await self.authorize(tickets[0], "192.0.2.1")).status, 200)
        response = await self.client.post(
            "/api/logout",
            headers=self.headers
            | {
                "Origin": "https://" + CONFIG["hostname"],
                "X-CSRF-Token": status["csrf"],
            },
        )
        self.assertEqual(response.status, 200)
        self.assertEqual((await self.authorize(tickets[1], "2001:db8::1")).status, 401)
        self.assertEqual(len(self.grants.run("42")), 1)

    async def test_cookie_binding_and_host_routing(self):
        response = await self.client.get(
            "/login", headers=self.headers, allow_redirects=False
        )
        state = parse_qs(urlparse(response.headers["Location"]).query)["state"][0]
        response = await self.client.get(
            "/oauth/callback?code=test&state=" + state,
            headers=self.headers,
            allow_redirects=False,
        )
        self.assertEqual(response.status, 403)
        self.assertEqual(self.token_calls, 0)
        response = await self.client.get("/", headers={"Host": CONFIG["ipv4Hostname"]})
        self.assertEqual(response.status, 404)
        response = await self.client.post(
            "/grant", json={"user": None, "ip": "192.0.2.1"}
        )
        self.assertEqual(response.status, 404)

    async def test_preflight_expired_ticket_and_private_broker_validation(self):
        response = await self.client.options(
            "/minecraft-access/authorize",
            headers={
                "Host": CONFIG["ipv4Hostname"],
                "Origin": "https://" + CONFIG["hostname"],
                "Access-Control-Request-Method": "POST",
                "Access-Control-Request-Headers": "content-type",
            },
        )
        self.assertEqual(response.status, 204)
        self.assertEqual(
            response.headers["Access-Control-Allow-Origin"],
            "https://" + CONFIG["hostname"],
        )
        self.assertEqual(
            response.headers["Access-Control-Allow-Headers"], "Content-Type"
        )
        await self.login()
        response, _ = await self.tickets()
        tickets = await response.json()
        with self.store.db:
            self.store.db.execute("UPDATE tickets SET expiry=0")
        self.assertEqual((await self.authorize(tickets[0], "192.0.2.1")).status, 403)
        for data in (
            {"user": None, "ip": "192.0.2.1"},
            {"user": "42", "ip": "::1%; flush ruleset"},
            {"user": "42", "ip": "192.0.2.1", "ports": [22]},
        ):
            async with self.http.post(
                self.broker_server.make_url("/grant"), json=data
            ) as response:
                self.assertEqual(response.status, 400)
        self.nft.assert_not_called()

    async def test_pending_login_limit(self):
        with self.store.db:
            self.store.db.executemany(
                "INSERT INTO logins VALUES (?, ?, ?)",
                [(str(index), "browser", 9999999999) for index in range(4096)],
            )
        response = await self.client.get(
            "/login", headers=self.headers, allow_redirects=False
        )
        self.assertEqual(response.status, 503)
        self.assertEqual(
            self.store.db.execute("SELECT count(*) FROM logins").fetchone()[0], 4096
        )


if __name__ == "__main__":
    unittest.main()
