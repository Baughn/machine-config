{ pkgs }:
# The agent board (machines/tsugumi/agent-board.nix): uid identity on the API socket, bearer
# tokens on TCP, a read-only HTML socket only the proxy can open, persistence across
# restarts, and the nightly copy. The lab sandbox's access is in minecraft-lab-vm.
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
    };
    environment.systemPackages = [ pkgs.curl pkgs.jq ];
  };
  testScript = ''
    import json, shlex

    API = "--unix-socket /run/agent-board/api.sock http://board"
    HTML = "--unix-socket /run/agent-board/html.sock http://board"
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

    with subtest("data survives a restart; the nightly copy works"):
        machine.succeed("systemctl restart agent-board.service")
        assert json.loads(curl("mclab", API + f"/threads/{thread}"))["thread"]["title"] == "Autosave spike"
        machine.succeed("systemctl start agent-board-backup.service")
        machine.succeed("test -s /var/lib/agent-board/backup/board-$(date -u +%F).db")
        machine.succeed("test \"$(stat -c %a /var/lib/agent-board)\" = 700")
  '';
}
