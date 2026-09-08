{ pkgs }:
let
  probe = pkgs.writeText "minecraft-probe.py" ''
    import socket
    import sys
    import threading
    import time

    mode = sys.argv[1]
    expected = mode == "allowed"
    for family, destination, sources in (
        (socket.AF_INET, "127.0.0.1", ("192.0.2.10", "192.0.2.11")),
        (socket.AF_INET6, "::1", ("2001:db8::10", "2001:db8::11")),
    ):
        for kind, port in ((socket.SOCK_STREAM, 25565), (socket.SOCK_STREAM, 25566),
                           (socket.SOCK_DGRAM, 24454), (socket.SOCK_STREAM, 25567)):
            with socket.socket(family, kind) as listener:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind((destination, port))
                listener.settimeout(0.3)
                if kind == socket.SOCK_STREAM:
                    listener.listen(5)
                for index, source in enumerate(sources):
                    with socket.socket(family, kind) as client:
                        client.settimeout(0.3)
                        client.bind((source, 0))
                        try:
                            if kind == socket.SOCK_STREAM:
                                client.connect((destination, port))
                                connection, _ = listener.accept()
                                connection.close()
                                received = True
                            else:
                                client.sendto(b"test", (destination, port))
                                received = listener.recv(16) == b"test"
                        except TimeoutError:
                            received = False
                        assert received == (port == 25567 or (expected and index == 0)), (mode, source, port)
  '';
  established = pkgs.writeText "minecraft-established.py" ''
    import socket
    import time
    from pathlib import Path
    with socket.socket() as listener, socket.socket() as client:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", 25565))
        listener.listen(1)
        client.bind(("192.0.2.10", 0))
        client.settimeout(2)
        client.connect(("127.0.0.1", 25565))
        peer, _ = listener.accept()
        with peer:
            Path("/run/established-ready").touch()
            while not Path("/run/check-established").exists():
                time.sleep(0.1)
            client.sendall(b"alive")
            assert peer.recv(5) == b"alive"
            Path("/run/established-ok").touch()
  '';
in pkgs.testers.runNixOSTest {
  name = "minecraft-access";
  nodes.machine = { lib, ... }: {
    imports = [ ../machines/tsugumi/minecraft-access.nix ];
    me.minecraft = {
      ports = { tcp = [ 25565 25566 ]; udp = [ 24454 ]; };
      access = {
        enable = true;
        clientId = "1"; guildId = "2"; roleId = "3";
        clientSecretFile = toString (pkgs.writeText "test-only-discord-secret" "fake-secret");
        leaseSeconds = 120;
      };
    };
    networking.firewall.allowedTCPPorts = [ 443 25567 ];
    environment.systemPackages = [ pkgs.curl pkgs.nftables pkgs.python3 pkgs.sqlite ];
    users.users.outsider.isNormalUser = true;
    services.caddy.virtualHosts = lib.genAttrs [ "minecraft.brage.info" "v4.brage.info" "v6.brage.info" ] (_: {
      extraConfig = lib.mkBefore "tls internal";
    });
  };
  testScript = ''
    import json
    import shlex
    import time

    machine.start(allow_reboot=True)
    machine.wait_for_unit("minecraft-access.service")
    machine.wait_for_unit("minecraft-access-firewall.service")
    machine.wait_for_unit("caddy.service")
    machine.wait_until_succeeds("test -S /run/minecraft-access/http.sock")
    machine.wait_until_succeeds("test -S /run/minecraft-access-firewall/http.sock")

    def addresses():
        for address in ("192.0.2.10/32", "192.0.2.11/32", "2001:db8::10/128", "2001:db8::11/128"):
            machine.succeed(f"ip address add {address} dev lo")

    addresses()
    def probe(mode):
        machine.succeed("python3 ${probe} " + mode)

    probe("blocked")
    grant_url = "curl -fsS --unix-socket /run/minecraft-access-firewall/http.sock http://localhost/grant -H 'Content-Type: application/json' -d "
    for address in ("192.0.2.10", "2001:db8::10"):
        command = grant_url + shlex.quote(json.dumps({"user": "42", "ip": address}))
        machine.fail("su -s /bin/sh outsider -c " + shlex.quote(command))
        machine.fail("su -s /bin/sh caddy -c " + shlex.quote(command))
        machine.succeed("su -s /bin/sh minecraft-access -c " + shlex.quote(command))
    probe("allowed")
    machine.succeed("systemctl reload firewall")
    probe("allowed")
    machine.succeed("systemctl stop firewall")
    probe("allowed")
    machine.succeed("systemctl start firewall minecraft-access-firewall")
    probe("allowed")

    # Exercise the real HTTPS reverse proxy with a test-only remembered session.
    # OAuth issuance and role checks use a mock Discord server in the unit tests.
    machine.succeed("systemctl stop minecraft-access")
    machine.succeed("sqlite3 /var/lib/minecraft-access/sessions.sqlite \"INSERT INTO sessions VALUES ('" + __import__('hashlib').sha256(b'test-session').hexdigest() + "', '42', 'test-csrf', unixepoch()+3600)\"")
    machine.succeed("systemctl start minecraft-access")
    main = "curl --noproxy '*' -kfsS --resolve minecraft.brage.info:443:127.0.0.1 -b '__Host-minecraft-session=test-session' https://minecraft.brage.info"
    machine.wait_until_succeeds(main + "/api/status")
    tickets = json.loads(machine.succeed(main + "/api/visit -X POST -H 'Origin: https://minecraft.brage.info' -H 'X-CSRF-Token: test-csrf'"))
    for ticket, address, dest in zip(tickets, ("192.0.2.10", "2001:db8::10"), ("127.0.0.1", "[::1]")):
        host = "v4.brage.info" if ticket["family"] == 4 else "v6.brage.info"
        result = json.loads(machine.succeed("curl --noproxy '*' -kfsS --interface " + address + " --resolve " + host + ":443:" + dest + " " + ticket["url"] + " -H 'Origin: https://minecraft.brage.info' -H 'Content-Type: application/json' -H 'X-Minecraft-Client-IP: 203.0.113.99' -H 'X-Forwarded-For: 203.0.113.99' -d " + shlex.quote(json.dumps({"ticket": ticket["ticket"]}))))
        assert result["ip"] == address, result

    # Reboot restores remaining lifetime, not a new lease.
    before = machine.succeed("sqlite3 /var/lib/minecraft-access-firewall/grants.sqlite 'select ip,expiry from grants order by ip'")
    machine.reboot()
    machine.wait_for_unit("minecraft-access-firewall.service")
    addresses()
    after = machine.succeed("sqlite3 /var/lib/minecraft-access-firewall/grants.sqlite 'select ip,expiry from grants order by ip'")
    assert before == after
    probe("allowed")

    # Shorten leases in persistent state, then reload and stop both applications.
    machine.succeed("sqlite3 /var/lib/minecraft-access-firewall/grants.sqlite 'update grants set expiry=unixepoch()+10'")
    machine.succeed("systemctl reload firewall")
    machine.succeed("python3 ${established} > /run/established.log 2>&1 &")
    machine.wait_until_succeeds("test -f /run/established-ready", timeout=15)
    machine.succeed("systemctl stop minecraft-access minecraft-access-firewall")
    machine.fail(main + "/oauth/callback?code=private-oauth-canary")
    assert "private-oauth-canary" not in machine.succeed("journalctl -u caddy --no-pager")
    time.sleep(11)
    machine.succeed("touch /run/check-established")
    machine.wait_until_succeeds("test -f /run/established-ok", timeout=15)
    probe("blocked")
    machine.succeed("systemctl reload firewall")
    probe("blocked")

    # Persistent-state failure retains the closed guard, including on a fresh boot.
    machine.succeed("rm /var/lib/minecraft-access-firewall/grants.sqlite")
    machine.succeed("echo broken > /var/lib/minecraft-access-firewall/grants.sqlite")
    machine.fail("systemctl reload firewall")
    probe("blocked")
    machine.reboot()
    machine.wait_for_unit("multi-user.target")
    addresses()
    probe("blocked")
  '';
}
