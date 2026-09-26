{ pkgs }:
# The lab's network fence and units (machines/tsugumi/minecraft-lab.nix): the
# namespace reaches the "internet" node's public address, never the host or a
# private range; the login port reaches a lab server; polkit and the sandbox
# hold. The clone helper itself is covered by minecraft-storage-vm.
let
  listener = port: "${pkgs.python3}/bin/python3 -m http.server ${toString port} --bind 0.0.0.0";
in
pkgs.testers.runNixOSTest {
  name = "minecraft-lab";
  nodes = {
    internet = { lib, ... }: {
      virtualisation.vlans = [ 1 2 ];
      networking.interfaces.eth2.ipv4.addresses = lib.mkForce [{ address = "198.51.100.1"; prefixLength = 24; }];
      networking.interfaces.eth1.ipv6.addresses = [{ address = "fd00::1"; prefixLength = 64; }];
      networking.firewall.enable = false;
      systemd.services.web = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig.ExecStart = listener 8000;
        serviceConfig.WorkingDirectory = "/etc";
      };
    };
    machine = { lib, pkgs, ... }: {
      imports = [ ../machines/tsugumi/minecraft-storage.nix ../machines/tsugumi/minecraft-lab.nix ];
      virtualisation.vlans = [ 1 2 ];
      # vlan 1 stands in for the LAN (private), vlan 2 for the internet.
      networking.interfaces.eth2.ipv4.addresses = lib.mkForce [{ address = "198.51.100.2"; prefixLength = 24; }];
      networking.defaultGateway = { address = "198.51.100.1"; interface = "eth2"; };
      networking.interfaces.eth1.ipv6.addresses = [{ address = "fd00::2"; prefixLength = 64; }];
      boot.supportedFilesystems = [ "zfs" ];
      networking.hostId = "feedbeef";
      services.zrepl = {
        enable = true;
        settings.jobs = [
          { name = "backup-sink"; type = "sink"; root_fs = "stash/zrepl";
            serve = { type = "local"; listener_name = "backup-sink"; }; }
          { name = "rpool"; type = "push"; filesystems."rpool/minecraft<" = true;
            connect = { type = "local"; listener_name = "backup-sink"; client_identity = "rpool"; };
            snapshotting.type = "manual";
            pruning = { keep_sender = [{ type = "last_n"; count = 1; }]; keep_receiver = [{ type = "last_n"; count = 1; }]; }; }
        ];
      };
      systemd.services.zrepl.wantedBy = lib.mkForce [ ];
      users.users.minecraft = { isNormalUser = true; };
      networking.firewall.allowedTCPPorts = [ 35565 ];
      # Stands in for production RCON, which vanilla binds on every address.
      systemd.services.fake-rcon = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig.ExecStart = listener 35565;
        serviceConfig.WorkingDirectory = "/etc";
      };
      environment.systemPackages = [ pkgs.curl ];
    };
  };
  testScript = { nodes, ... }:
    let
      sandbox = nodes.machine.me.minecraft.lab.serviceConfig;
      props = pkgs.lib.concatStringsSep " " ([
        "-p User=mclab" "-p NetworkNamespacePath=${sandbox.NetworkNamespacePath}"
        "-p ProtectHome=yes" "-p NoNewPrivileges=yes" "-p PrivateTmp=yes"
      ] ++ map (bind: "-p BindReadOnlyPaths=${bind}") sandbox.BindReadOnlyPaths);
    in ''
      import shlex

      start_all()
      internet.wait_for_unit("web.service")
      machine.wait_for_unit("minecraft-lab-pasta.service")
      machine.wait_for_unit("fake-rcon.service")
      machine.wait_for_open_port(35565)

      def lab(command, success=True):
          """Run a command as the lab's units do: as mclab, in the namespace and sandbox."""
          run = "systemd-run --quiet --wait --pipe --collect ${props} -p Environment=PATH=/run/current-system/sw/bin -- bash -c " + shlex.quote(command)
          return (machine.succeed if success else machine.fail)(run)

      def reach(address, port):
          return f"curl -sf --max-time 5 -o /dev/null http://{address}:{port}/"

      with subtest("the lab has its own namespace"):
          host = machine.succeed("readlink /proc/1/ns/net").strip()
          assert lab("readlink /proc/self/ns/net").strip() != host
          assert "198.51.100.2" in lab("ip -4 addr")
          assert "192.168.1.2" not in lab("ip -4 addr")

      with subtest("the namespace reaches the internet, not the host or private ranges"):
          lab(reach("198.51.100.1", 8000))
          internet.succeed(reach("192.168.1.1", 8000))
          lab(reach("192.168.1.1", 8000), success=False)
          for address in ("127.0.0.1", "198.51.100.2", "192.168.1.2", "10.0.2.15"):
              lab(reach(address, 35565), success=False)
          # The fence covers mclab outside the namespace too.
          machine.fail("sudo -u mclab " + reach("127.0.0.1", 35565))
          machine.succeed(reach("127.0.0.1", 35565))
          # Socketless traffic (NDP here) isn't mclab's, so the fence lets it be.
          machine.succeed("ping -6 -c1 -W5 fd00::1")

      with subtest("the sandbox hides homes and pins resolv.conf"):
          machine.succeed("mkdir -p /home/minecraft/erisia", "echo token > /home/minecraft/erisia/secret",
                          "chmod 705 /home/minecraft", "chmod 755 /home/minecraft/erisia")
          machine.succeed("sudo -u mclab cat /home/minecraft/erisia/secret")
          lab("cat /home/minecraft/erisia/secret", success=False)
          assert "1.1.1.1" in lab("cat /etc/resolv.conf")
          lab("sudo -n true", success=False)

      with subtest("the helper answers the lab under NoNewPrivileges"):
          assert "0 of 3 lab clones" in lab("minecraft-lab list")
          lab("minecraft-lab destroy nothing", success=False)

      with subtest("lab servers: polkit, the namespace and the login port"):
          machine.succeed("mkdir -p /srv/minecraft-lab/x/server",
                          "printf '#!/bin/sh\\nexec ${pkgs.python3}/bin/python3 -m http.server 25565 --bind 0.0.0.0\\n' > /srv/minecraft-lab/x/server/start.py",
                          "printf '#!/bin/sh\\nexit 0\\n' > /srv/minecraft-lab/x/control.sh",
                          "chmod +x /srv/minecraft-lab/x/server/start.py /srv/minecraft-lab/x/control.sh",
                          "chown -R mclab:mclab /srv/minecraft-lab/x")
          machine.succeed("sudo -u mclab systemctl start minecraft-lab@x")
          machine.wait_until_succeeds(reach("127.0.0.1", 25665))
          # It listens in the namespace, not on the host.
          machine.fail(reach("127.0.0.1", 25565))
          lab(reach("127.0.0.1", 25565))
          assert "mclab.slice" in machine.succeed("systemctl show -p Slice minecraft-lab@x")
          machine.fail("sudo -u mclab systemctl start fake-rcon")
          machine.fail("sudo -u mclab systemctl stop minecraft-lab-pasta")
          machine.fail("sudo -u mclab systemctl start 'minecraft@x'")
          machine.succeed("sudo -u mclab systemctl stop minecraft-lab@x")
    '';
}
