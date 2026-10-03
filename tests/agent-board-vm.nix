{ pkgs }:
# The agent board (machines/tsugumi/agent-board.nix): uid identity on the API socket, bearer
# tokens on TCP, a read-only HTML socket only the proxy can open, persistence across
# restarts, the nightly copy, and login links from Discord's /board. The lab sandbox's access is in minecraft-lab-vm.
pkgs.testers.runNixOSTest {
  name = "agent-board";
  nodes.machine = { pkgs, ... }: {
    imports = [ ../machines/tsugumi/agent-board.nix ];
    users.users.minecraft = { isNormalUser = true; };
    users.users.mclab = { isSystemUser = true; group = "mclab"; };
    users.groups.mclab = { };
    # Stands in for Caddy, which owns the HTML socket's group in production.
    users.groups.caddy = { };
    users.users.proxy = { isSystemUser = true; group = "caddy"; };
    # Stands in for agent-board-discord, which needs a real bot token.
    users.users.poller = { isSystemUser = true; group = "poller"; };
    users.groups.poller = { };
    me.agentBoard = {
      users.poller = "discord";
      httpAddress = "127.0.0.1:8740";
      tokens.saya = pkgs.writeText "token" "test-token-0123456789abcdef0123456789";
      # The public half of the test key below (seed 0x07 * 32).
      discordPublicKey = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
    };
    environment.systemPackages = [ pkgs.curl pkgs.jq pkgs.openssl ];
    environment.etc."discord-test-key.pem".text = ''
      -----BEGIN PRIVATE KEY-----
      MC4CAQAwBQYDK2VwBCIEIAcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcH
      -----END PRIVATE KEY-----
    '';
  };
  testScript = ''
    import json, shlex

    API = "--unix-socket /run/agent-board/api.sock http://board"
    # Caddy adds the header once Authelia has passed the request.
    HTML = "--unix-socket /run/agent-board/html.sock -H 'X-Board-Authelia: 1' http://board"
    BARE_HTML = "--unix-socket /run/agent-board/html.sock http://board"
    INTERACTIONS = "--unix-socket /run/agent-board/interactions.sock http://board/discord/interactions"
    TOKEN = "test-token-0123456789abcdef0123456789"

    def curl(user, args, success=True):
        command = "sudo -u " + user + " curl -s --fail-with-body " + args
        return (machine.succeed if success else machine.fail)(command)

    def post(user, path, body):
        return json.loads(curl(user, "-X POST -H 'content-type: application/json' -d "
                               + shlex.quote(json.dumps(body)) + " " + API + path))

    start_all()
    machine.wait_for_unit("sockets.target")
    machine.wait_for_unit("agent-board.service")

    with subtest("callers are identified by uid; others are refused"):
        assert json.loads(curl("minecraft", API + "/whoami"))["agent"] == "tsugumi-minecraft"
        assert json.loads(curl("mclab", API + "/whoami"))["agent"] == "tsugumi-lab"
        assert "403" in machine.succeed("curl -s -o /dev/null -w '%{http_code}' " + API + "/whoami")
        curl("proxy", API + "/threads", success=False)

    with subtest("threads, posts and search"):
        created = post("mclab", "/threads", {
            "title": "Autosave spike", "tags": ["perf"], "summary": "Buffer the writes.",
            "post": {"body": "Hello <script>alert(1)</script>", "ask": "tsugumi-minecraft",
                     "attachments": [{"name": "trace.txt", "content": "write() x 32500"}]}})
        thread, ask = created["thread"], created["post"]
        post("minecraft", f"/threads/{thread}/posts", {"body": "Confirmed in prod.", "reply_to": ask})
        view = json.loads(curl("minecraft", API + f"/threads/{thread}"))
        assert [p["author"] for p in view["posts"]] == ["tsugumi-lab", "tsugumi-minecraft"]
        assert view["posts"][0]["answered"] is True
        hits = json.loads(curl("mclab", API + "/search?q=prod"))
        assert hits[0]["author"] == "tsugumi-minecraft", hits

    with subtest("bearer tokens on TCP"):
        machine.fail("curl -sf http://127.0.0.1:8740/whoami")
        machine.fail("curl -sf -H 'Authorization: Bearer wrong' http://127.0.0.1:8740/whoami")
        who = machine.succeed(f"curl -sf -H 'Authorization: Bearer {TOKEN}' http://127.0.0.1:8740/whoami")
        assert json.loads(who)["agent"] == "saya"

    with subtest("the HTML is read-only and only the proxy can open it"):
        page = curl("proxy", HTML + f"/t/{thread}")
        assert "Autosave spike" in page and "trace.txt" in page
        assert "<script>alert" not in page and "&lt;script&gt;" in page
        headers = curl("proxy", "-D - -o /dev/null " + HTML + "/")
        assert "default-src 'none'" in headers, headers
        assert "Autosave spike" in curl("proxy", HTML + "/search?q=buffer")
        status = curl("proxy", "-o /dev/null -w '%{http_code}' -X POST " + HTML + f"/t/{thread}", success=False)
        assert status.strip() == "405", status
        curl("minecraft", HTML + "/", success=False)

    with subtest("status cards show on the status page"):
        curl("mclab", "-X PUT -H 'content-type: application/json' -d "
             + shlex.quote(json.dumps({"title": "Lab (timer)", "state": "1 server running", "ttl": 180,
                                       "lines": [{"text": "server x: active", "level": "info"}]}))
             + " " + API + "/status/lab")
        page = curl("proxy", HTML + "/status")
        assert "Lab (timer)" in page and "server x: active" in page and "tsugumi-lab/lab" in page, page
        assert 'http-equiv="refresh"' in page, page

    with subtest("only the poller writes the Discord archive; everyone can read it"):
        batch = {"messages": [{"id": 1555569477968724201, "channel": 1553121660532432926,
                               "author": "baughn", "author_kind": "human", "created": 1790947064,
                               "content": "Cleaned-up history makes sense.",
                               "attachments": [{"name": "plan.md", "size": 9, "text": "use restic"}],
                               "url": "https://discord.com/channels/1/2/1555569477968724201"}]}
        machine.fail("sudo -u mclab curl -sf -X POST -H 'content-type: application/json' -d "
                     + shlex.quote(json.dumps(batch)) + " " + API + "/discord")
        assert post("poller", "/discord", batch)["inserted"] == 1
        hits = json.loads(curl("mclab", API + "/search?q=restic&kind=discord"))
        assert hits[0]["id"] == 1555569477968724201, hits
        context = json.loads(curl("minecraft", API + "/discord/1555569477968724201"))
        assert context["messages"][0]["author"] == "baughn"
        page = curl("proxy", HTML + "/d/1555569477968724201")
        assert "Cleaned-up history" in page and "plan.md" in page
        assert "Cleaned-up history" in curl("proxy", HTML + "/day/2026-10-02")

    def interaction(payload, sign=True):
        """POSTs a /board interaction signed like Discord's; returns (status, body)."""
        machine.succeed("cat > /tmp/body <<'EOF'\n" + json.dumps(payload) + "\nEOF")
        machine.succeed("truncate -s -1 /tmp/body; date +%s | tr -d '\\n' > /tmp/ts")
        machine.succeed("cat /tmp/ts /tmp/body > /tmp/msg")
        signature = machine.succeed(
            "openssl pkeyutl -sign -inkey /etc/discord-test-key.pem -rawin -in /tmp/msg"
            " | od -An -tx1 | tr -d ' \\n'").strip()
        if not sign:
            signature = "00" * 64
        out = machine.succeed(
            "sudo -u proxy curl -s -w '\\n%{http_code}' -H 'content-type: application/json'"
            f" -H 'x-signature-ed25519: {signature}' -H \"x-signature-timestamp: $(cat /tmp/ts)\""
            " --data-binary @/tmp/body " + INTERACTIONS)
        body, status = out.rsplit("\n", 1)
        return int(status), body

    with subtest("/board on Discord hands admins a one-time login link"):
        assert "401" in curl("proxy", "-o /dev/null -w '%{http_code}' " + BARE_HTML + "/status", success=False)
        status, _ = interaction({"type": 1}, sign=False)
        assert status == 401, status
        status, body = interaction({"type": 1})
        assert status == 200 and json.loads(body) == {"type": 1}, body
        member = lambda roles: {"type": 2, "guild_id": "153634590190206977", "data": {"name": "board"},
                                "member": {"user": {"id": "42", "username": "vindex"}, "roles": roles}}
        status, body = interaction(member(["1"]))
        assert "admins only" in json.loads(body)["data"]["content"], body
        status, body = interaction(member(["280158066195038208"]))
        reply = json.loads(body)["data"]
        assert reply["flags"] == 64 | 4, reply
        link = reply["content"].split("https://agents.brage.info")[1].split(">")[0]
        assert "vindex" in curl("proxy", BARE_HTML + link)
        headers = curl("proxy", "-D - -o /dev/null -X POST " + BARE_HTML + link)
        cookie = [line for line in headers.splitlines() if line.lower().startswith("set-cookie:")][0]
        session = cookie.split(":", 1)[1].split(";")[0].strip()
        page = curl("proxy", f"-H 'Cookie: {session}' " + BARE_HTML + "/status")
        assert "Agent board" in page
        curl("proxy", "-X POST " + BARE_HTML + link, success=False)
        assert "vindex" in machine.succeed("agent-board-sessions list")
        machine.succeed("agent-board-sessions revoke all")
        curl("proxy", f"-H 'Cookie: {session}' " + BARE_HTML + "/status", success=False)

    with subtest("data survives a restart; the nightly copy works"):
        machine.succeed("systemctl restart agent-board.service")
        assert json.loads(curl("mclab", API + f"/threads/{thread}"))["thread"]["title"] == "Autosave spike"
        machine.succeed("systemctl start agent-board-backup.service")
        machine.succeed("test -s /var/lib/agent-board/backup/board-$(date -u +%F).db")
        machine.succeed("test \"$(stat -c %a /var/lib/agent-board)\" = 700")
  '';
}
