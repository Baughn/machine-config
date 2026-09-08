{ pkgs }:

let
  prefixUpdater = pkgs.writeScript "test-victron-prefixes" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings [ "@nft@" ] [ "${pkgs.nftables}/bin/nft" ]
      (builtins.readFile ../machines/tsugumi/starlink-prefixes.py)
  );
  checkIngress = pkgs.writeText "check-victron-ingress.py" ''
    import socket
    import sys

    allow_listed = sys.argv[1] == "listed"
    for family, destination, sources in (
        (socket.AF_INET, "127.0.0.1", ("8.8.8.8", "1.1.1.1")),
        (socket.AF_INET6, "::1", ("2001:4860::1", "2606:4700::1")),
    ):
        with socket.socket(family, socket.SOCK_DGRAM) as receiver:
            receiver.bind((destination, 9099))
            receiver.settimeout(0.3)
            for index, source in enumerate(sources):
                with socket.socket(family, socket.SOCK_DGRAM) as sender:
                    sender.bind((source, 0))
                    sender.sendto(b"test", (destination, 9099))
                    try:
                        received = receiver.recv(16) == b"test"
                    except TimeoutError:
                        received = False
                    assert received == (allow_listed and index == 0), source
  '';
in
pkgs.testers.runNixOSTest {
  name = "local-web-access";
  nodes.machine = { lib, ... }: {
    imports = [
      ../machines/tsugumi/local-web-access.nix
      ../machines/tsugumi/static-web.nix
    ];
    users.users = lib.genAttrs [ "caddy" "silverbullet" "minecraft" "outsider" ] (_: {
      isNormalUser = true;
    });
    environment.systemPackages = [ pkgs.curl pkgs.nftables ];
    networking.firewall.extraCommands = "${prefixUpdater} --restore";
    systemd.tmpfiles.rules = [
      "d /srv/svein 0755 root root -"
      "d /srv/minecraft 0755 root root -"
      "d /srv/aquagon 0755 root root -"
      "f /srv/svein/index.html 0644 root root - public-content"
      "d /var/lib/caddy 0755 root root -"
      "f /var/lib/caddy/test-secret 0644 root root - private-content"
      "L /srv/svein/leak - - - - /var/lib/caddy/test-secret"
    ];
    systemd.services = builtins.listToAttrs (map (port: {
      name = "backend-${toString port}";
      value = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          User = if port == 3000 then "silverbullet" else "minecraft";
          ExecStart = "${pkgs.python3}/bin/python3 -m http.server ${toString port} --bind :: --directory /run";
        };
      };
    }) [ 3000 8123 8124 ]);
  };
  testScript = ''
    import shlex

    machine.start()
    machine.wait_for_unit("caddy-static.service")
    try:
        machine.wait_until_succeeds("test -S /run/caddy-static/http.sock", timeout=20)
    except Exception:
        machine.log(machine.succeed("journalctl -u caddy-static --no-pager"))
        raise
    static_request = "curl --fail --max-time 2 -s --unix-socket /run/caddy-static/http.sock -H 'Host: brage.info' http://localhost/"
    assert "public-content" in machine.succeed(f"su -s /bin/sh caddy -c {shlex.quote(static_request)}")
    machine.fail(f"su -s /bin/sh outsider -c {shlex.quote(static_request)}")
    machine.fail(f"su -s /bin/sh caddy -c {shlex.quote(static_request + 'leak')}")
    for port in (3000, 8123, 8124):
        machine.wait_for_unit(f"backend-{port}.service")
        machine.wait_for_open_port(port)

    for address in ("8.8.8.8/32", "1.1.1.1/32", "2001:4860::1/128", "2606:4700::1/128"):
        machine.succeed(f"ip address add {address} dev lo")

    def check_ingress(mode="listed"):
        machine.succeed(f"${pkgs.python3}/bin/python3 ${checkIngress} {mode}")

    # No cache at first boot: deny both families, despite loopback acceptance.
    check_ingress("empty")
    machine.succeed("mkdir -p /var/lib/victron-prefixes")
    machine.succeed("echo '[\"8.8.8.0/24\", \"2001:4860::/32\"]' > /var/lib/victron-prefixes/prefixes.json")
    machine.succeed("${prefixUpdater} --restore")

    def check_access():
        for host in ("127.0.0.1", "[::1]"):
            for port in (3000, 8123, 8124):
                request = f"curl --noproxy '*' --fail --max-time 2 -s http://{host}:{port}/"
                owner = "silverbullet" if port == 3000 else "minecraft"
                for user in ("root", "caddy", owner):
                    machine.succeed(f"su -s /bin/sh {user} -c {shlex.quote(request)}")
                machine.fail(f"su -s /bin/sh outsider -c {shlex.quote(request)}")
                if port == 3000:
                    machine.fail(f"su -s /bin/sh minecraft -c {shlex.quote(request)}")

    check_access()
    check_ingress()
    # A firewall reload/stop must not expose the unauthenticated backends.
    machine.succeed("systemctl reload firewall")
    check_access()
    check_ingress()
    machine.succeed("systemctl stop firewall")
    machine.succeed("nft list table inet local_web_access")
    check_access()
    check_ingress()
    machine.succeed("systemctl start firewall")
    check_access()
    check_ingress()
    machine.succeed("echo broken > /var/lib/victron-prefixes/prefixes.json")
    machine.succeed("${prefixUpdater} --restore")
    check_ingress("empty")
  '';
}
