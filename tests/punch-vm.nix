{ pkgs }:
let
  token = pkgs.writeText "test-only-broker-token" "0123456789abcdef0123456789abcdef";
  remoteProbe = pkgs.writeText "punch-remote-probe.py" ''
    import socket
    import sys
    for family, destination, source in ((socket.AF_INET, "127.0.0.1", "192.0.2.10"), (socket.AF_INET6, "::1", "2001:db8::10")):
        for port in (27015, 27016, 25565):
            with socket.socket(family, socket.SOCK_DGRAM) as listener, socket.socket(family, socket.SOCK_DGRAM) as client:
                listener.bind((destination, port))
                listener.settimeout(0.3)
                client.bind((source, 0))
                client.sendto(b"probe", (destination, port))
                try:
                    received = listener.recv(16) == b"probe"
                except TimeoutError:
                    received = False
                assert received == (port == 25565 or (sys.argv[1] == "allowed" and family == socket.AF_INET6)), (family, port)
  '';
  probe = pkgs.writeText "minecraft-probe.py" ''
    import socket
    import sys
    import threading
    import time

    mode = sys.argv[1]
    expected = mode == "allowed"
    for family, destination, sources in (
        (socket.AF_INET, "127.0.0.1", ("192.0.2.10", "192.0.2.11", "192.0.2.12", "192.0.2.13")),
        (socket.AF_INET6, "::1", ("2001:db8::10", "2001:db8::11", "2001:db8::12", "2001:db8::13")),
    ):
        for kind, port in ((socket.SOCK_STREAM, 25565), (socket.SOCK_STREAM, 25566),
                           (socket.SOCK_DGRAM, 24454), (socket.SOCK_STREAM, 25568),
                           (socket.SOCK_DGRAM, 27016), (socket.SOCK_STREAM, 25567)):
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
                        permitted = ({25565, 25566, 24454}, {25566, 25568, 27016}, {25565, 25566, 24454, 25568, 27016}, set())[index]
                        assert received == (port == 25567 or (expected and port in permitted)), (mode, source, port)
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
in
pkgs.testers.runNixOSTest {
  name = "punch";
  nodes.machine = { lib, ... }: {
    imports = [ ../modules/punch.nix ];
    me.punch = {
      groups.minecraft = {
        label = "Minecraft";
        roleIds = [ "3" ];
        ports = {
          tcp = [
            25565
            25566
          ];
          udp = [ 24454 ];
        };
      };
      groups.stationeers = {
        label = "Stationeers";
        roleIds = [ "4" ];
        ports = {
          tcp = [
            25566
            25568
          ];
          udp = [ 27016 ];
        };
      };
      groups.remote = {
        label = "Remote Stationeers";
        roleIds = [ "5" ];
        host = "remote";
        addressFamilies = [ 6 ];
        ports.udp = [
          27015
          27016
        ];
      };
      remoteBrokers.remote = {
        url = "http://192.168.1.2:9781";
        tokenFile = toString token;
      };
      enable = true;
      clientId = "1";
      guildId = "2";
      clientSecretFile = toString (pkgs.writeText "test-only-discord-secret" "fake-secret");
      leaseSeconds = 300;
    };
    networking.firewall.allowedTCPPorts = [
      443
      25567
    ];
    environment.systemPackages = [
      pkgs.curl
      pkgs.nftables
      pkgs.python3
      pkgs.sqlite
    ];
    users.users.outsider.isNormalUser = true;
    services.caddy.virtualHosts =
      lib.genAttrs [ "punch.brage.info" "v4.brage.info" "v6.brage.info" ]
        (_: {
          extraConfig = lib.mkBefore "tls internal";
        });
  };
  nodes.remote = { ... }: {
    imports = [ ../modules/punch.nix ];
    me.punch = {
      enable = true;
      frontendEnable = false;
      groups.remote = {
        label = "Remote Stationeers";
        roleIds = [ "5" ];
        addressFamilies = [ 6 ];
        ports.udp = [
          27015
          27016
        ];
      };
      listenAddress = "192.168.1.2";
      tokenFile = toString token;
    };
    networking.firewall.allowedTCPPorts = [ 9781 ];
    environment.systemPackages = [
      pkgs.curl
      pkgs.nftables
      pkgs.python3
      pkgs.sqlite
    ];
  };
  testScript = ''
    import json
    import shlex
    import time

    remote.start()
    remote.wait_for_unit("punch-firewall.service")
    remote.succeed("ip address add 192.0.2.10/32 dev lo")
    remote.succeed("ip address add 2001:db8::10/128 dev lo")
    remote.succeed("python3 ${remoteProbe} blocked")
    machine.start(allow_reboot=True)
    machine.wait_for_unit("punch.service")
    machine.wait_for_unit("punch-firewall.service")
    machine.wait_for_unit("caddy.service")
    machine.wait_until_succeeds("test -S /run/punch/http.sock")
    machine.wait_until_succeeds("test -S /run/punch-firewall/http.sock")

    def addresses():
        for address in ("192.0.2.10/32", "192.0.2.11/32", "192.0.2.12/32", "192.0.2.13/32", "2001:db8::10/128", "2001:db8::11/128", "2001:db8::12/128", "2001:db8::13/128"):
            machine.succeed(f"ip address add {address} dev lo")

    addresses()
    def probe(mode):
        machine.succeed("python3 ${probe} " + mode)

    probe("blocked")
    grant_url = "curl -fsS --unix-socket /run/punch-firewall/http.sock http://localhost/grant -H 'Content-Type: application/json' -d "
    for address in ("192.0.2.10", "2001:db8::10"):
        command = grant_url + shlex.quote(json.dumps({"user": "42", "ip": address, "groups": ["minecraft"]}))
        machine.fail("su -s /bin/sh outsider -c " + shlex.quote(command))
        machine.fail("su -s /bin/sh caddy -c " + shlex.quote(command))
        machine.succeed("su -s /bin/sh punch -c " + shlex.quote(command))
    for user, suffix, groups in (("43", "11", ["stationeers"]), ("44", "12", ["minecraft", "stationeers"])):
        for prefix in ("192.0.2.", "2001:db8::"):
            machine.succeed(grant_url + shlex.quote(json.dumps({"user": user, "ip": prefix + suffix, "groups": groups})))
    probe("allowed")
    machine.succeed("systemctl reload firewall")
    probe("allowed")
    machine.succeed("systemctl stop firewall")
    probe("allowed")
    machine.succeed("systemctl start firewall punch-firewall")
    probe("allowed")

    # Exercise the real HTTPS reverse proxy with a test-only remembered session.
    # OAuth issuance and role checks use a mock Discord server in the unit tests.
    machine.succeed("systemctl stop punch")
    session_id = __import__('hashlib').sha256(b'test-session').hexdigest()
    sql = "INSERT INTO sessions VALUES ('" + session_id + "', '42', 'test-csrf', unixepoch()+3600, '" + json.dumps(["3", "5"]) + "')"
    machine.succeed("sqlite3 /var/lib/punch/sessions.sqlite " + shlex.quote(sql))
    machine.succeed("systemctl start punch")
    main = "curl --noproxy '*' -kfsS --resolve punch.brage.info:443:127.0.0.1 -b '__Host-punch-session=test-session' https://punch.brage.info"
    machine.wait_until_succeeds(main + "/api/status")
    tickets = json.loads(machine.succeed(main + "/api/visit -X POST -H 'Origin: https://punch.brage.info' -H 'X-CSRF-Token: test-csrf'"))
    for ticket, address, dest in zip(tickets, ("192.0.2.10", "2001:db8::10"), ("127.0.0.1", "[::1]")):
        host = "v4.brage.info" if ticket["family"] == 4 else "v6.brage.info"
        result = json.loads(machine.succeed("curl --noproxy '*' -kfsS --interface " + address + " --resolve " + host + ":443:" + dest + " " + ticket["url"] + " -H 'Origin: https://punch.brage.info' -H 'Content-Type: application/json' -H 'X-Punch-Client-IP: 203.0.113.99' -H 'X-Forwarded-For: 203.0.113.99' -d " + shlex.quote(json.dumps({"ticket": ticket["ticket"]}))))
        assert result["ip"] == address, result
        assert result["errors"] == [], result
        assert ("remote" in result["enabled"]) == (ticket["family"] == 6), result

    remote.succeed("python3 ${remoteProbe} allowed")
    assert "remote" not in machine.succeed("sqlite3 /var/lib/punch-firewall/grants.sqlite 'select group_id from grants'")
    assert remote.succeed("sqlite3 /var/lib/punch-firewall/grants.sqlite 'select group_id,ip from grants'").strip() == "remote|2001:db8::10"
    machine.succeed("test $(curl -s -o /dev/null -w '%{http_code}' http://192.168.1.2:9781/list -d '{\"user\":\"42\"}') = 401")
    remote.succeed("systemctl stop punch-firewall")
    status = json.loads(machine.succeed(main + "/api/status"))
    assert status["grants"] and status["errors"][0]["groups"] == ["remote"], status
    remote.succeed("systemctl start punch-firewall")

    # Reboot restores remaining lifetime, not a new lease.
    before = machine.succeed("sqlite3 /var/lib/punch-firewall/grants.sqlite 'select ip,expiry from grants order by ip'")
    machine.reboot()
    machine.wait_for_unit("punch-firewall.service")
    addresses()
    after = machine.succeed("sqlite3 /var/lib/punch-firewall/grants.sqlite 'select ip,expiry from grants order by ip'")
    assert before == after
    probe("allowed")

    # Shorten leases in persistent state, then reload and stop both applications.
    machine.succeed("sqlite3 /var/lib/punch-firewall/grants.sqlite 'update grants set expiry=unixepoch()+10'")
    machine.succeed("systemctl reload firewall")
    machine.succeed("python3 ${established} > /run/established.log 2>&1 &")
    machine.wait_until_succeeds("test -f /run/established-ready", timeout=15)
    machine.succeed("systemctl stop punch punch-firewall")
    machine.fail(main + "/oauth/callback?code=private-oauth-canary")
    assert "private-oauth-canary" not in machine.succeed("journalctl -u caddy --no-pager")
    time.sleep(11)
    machine.succeed("touch /run/check-established")
    machine.wait_until_succeeds("test -f /run/established-ok", timeout=15)
    probe("blocked")
    machine.succeed("systemctl reload firewall")
    probe("blocked")

    # Persistent-state failure retains the closed guard, including on a fresh boot.
    machine.succeed("rm /var/lib/punch-firewall/grants.sqlite")
    machine.succeed("echo broken > /var/lib/punch-firewall/grants.sqlite")
    machine.fail("systemctl reload firewall")
    probe("blocked")
    machine.reboot()
    machine.wait_for_unit("multi-user.target")
    addresses()
    probe("blocked")
  '';
}
