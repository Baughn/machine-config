{ pkgs }:
let
  # Discord stand-in: appends each webhook body to a file.
  stub = pkgs.writeScript "webhook-stub.py" ''
    #!${pkgs.python3}/bin/python3
    from http.server import BaseHTTPRequestHandler, HTTPServer
    class Hook(BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            with open("/var/lib/webhook-stub/posts", "ab") as posts:
                posts.write(body + b"\n")
            self.send_response(204)
            self.end_headers()
    HTTPServer(("127.0.0.1", 8080), Hook).serve_forever()
  '';
  idle = pkgs.writeScript "update-and-start.sh" ''
    #!/usr/bin/env bash
    exec sleep infinity
  '';
in
pkgs.testers.runNixOSTest {
  name = "minecraft-watch";
  nodes.machine = { lib, ... }: {
    imports = [
      ../machines/tsugumi/minecraft-servers.nix
      ../machines/tsugumi/minecraft-watch.nix
    ];
    virtualisation.emptyDiskImages = [ 1024 1024 ];
    boot.supportedFilesystems = [ "zfs" ];
    networking.hostId = "feedbeef";
    users.users.minecraft = { isNormalUser = true; uid = 1018; linger = true; };
    # Fails at boot: its directory doesn't exist yet.
    me.minecraft.autostart = [ "w" ];
    me.minecraft.watch = {
      webhookFile = toString (pkgs.writeText "webhook" "http://127.0.0.1:8080/hook");
      settings = {
        snapshotAge = 45 * 60;
        replicaAge = 2 * 3600;
        leaseAge = 10 * 60;
        bootGrace = 0;
        loopRestarts = 3;
        loopWindow = 3600;
        confirm = 2;
      };
    };
    systemd.services.webhook-stub = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig = { ExecStart = stub; StateDirectory = "webhook-stub"; };
    };
    services.zrepl = {
      enable = true;
      settings.jobs = [
        {
          name = "backup-sink";
          type = "sink";
          root_fs = "stash/zrepl";
          serve = { type = "local"; listener_name = "backup-sink"; };
          recv.placeholder.encryption = "off";
        }
        {
          name = "rpool";
          type = "push";
          connect = { type = "local"; listener_name = "backup-sink"; client_identity = "rpool"; };
          filesystems."rpool/minecraft<" = true;
          snapshotting.type = "manual";
          pruning = {
            keep_sender = [{ type = "last_n"; count = 4; }];
            keep_receiver = [{ type = "last_n"; count = 4; }];
          };
        }
      ];
    };
  };
  testScript = ''
    import json

    def posts():
        text = machine.succeed("cat /var/lib/webhook-stub/posts 2>/dev/null || true")
        return [json.loads(line)["content"] for line in text.splitlines()]

    def watch(times=1):
        for _ in range(times):
            machine.succeed("systemctl start minecraft-watch")

    def expect(count, needle):
        found = posts()
        assert len(found) == count, found
        assert needle in found[-1], found[-1]

    def replicate(snap):
        machine.succeed(f"zfs snapshot rpool/minecraft/w1@{snap}")
        machine.succeed("zrepl signal wakeup rpool")
        machine.wait_until_succeeds(f"zfs list stash/zrepl/rpool/rpool/minecraft/w1@{snap}", timeout=60)

    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("webhook-stub.service")
    machine.succeed("systemctl stop zrepl")
    machine.succeed("zpool create rpool /dev/vdb", "zpool create stash /dev/vdc")
    machine.succeed("zfs create -o mountpoint=none rpool/minecraft", "zfs snapshot rpool/minecraft@root",
                    "zfs create -o mountpoint=none rpool/minecraft/w1",
                    "zfs create -p -o mountpoint=none stash/zrepl")
    machine.succeed("systemctl start zrepl")
    replicate("zrepl_a")

    with subtest("a world that is down fires after two runs"):
        machine.fail("systemctl is-active minecraft@w.service")
        watch()
        assert posts() == [], posts()
        watch()
        expect(1, "world:w")
        watch()
        assert len(posts()) == 1

    with subtest("the world coming up resolves it"):
        machine.succeed(
            "install -d -o minecraft /home/minecraft/w /home/minecraft/w/world /home/minecraft/w/mods",
            "touch /home/minecraft/w/server.nix-target",
            "install -m 755 ${idle} /home/minecraft/w/update-and-start.sh",
            "ln -s /run/current-system/sw/bin/true /home/minecraft/w/control.sh",
            "systemctl restart minecraft@w.service",
        )
        machine.wait_for_unit("minecraft@w.service")
        watch()
        expect(2, "resolved: `world:w`")
        machine.succeed("minecraft-watch status")

    with subtest("a stale snapshot fires and a fresh one resolves"):
        machine.succeed("date -s '+1 hour'")
        watch(2)
        expect(3, "snapshot:w1")
        assert "1 h 0 min old" in posts()[-1] or "1 h 1 min old" in posts()[-1], posts()[-1]
        machine.fail("minecraft-watch status")
        replicate("zrepl_b")
        watch()
        expect(4, "resolved: `snapshot:w1`")

    with subtest("a stuck save lease"):
        machine.succeed("install -d /run/minecraft-save-hook/w1",
                        "touch -d '-20 min' /run/minecraft-save-hook/w1/pending.json")
        watch(2)
        expect(5, "lease:w1")
        machine.succeed("rm /run/minecraft-save-hook/w1/pending.json")
        watch()
        expect(6, "resolved: `lease:w1`")

    with subtest("an interrupted rollback"):
        machine.succeed("install -d -m 700 /var/lib/minecraft-storage",
                        "echo {} > /var/lib/minecraft-storage/rollback.json",
                        "systemctl stop zrepl")
        watch(2)
        expect(7, "zrepl:rollback")
        assert "zrepl:active" not in posts()[-1]
        machine.succeed("rm /var/lib/minecraft-storage/rollback.json", "systemctl start zrepl")
        watch()
        expect(8, "resolved: `zrepl:rollback`")

    with subtest("a failed webhook post is retried"):
        machine.succeed("systemctl stop webhook-stub", "systemctl stop zrepl")
        machine.succeed("systemctl start minecraft-watch")
        machine.fail("systemctl start minecraft-watch")
        machine.succeed("systemctl start webhook-stub")
        machine.wait_for_open_port(8080)
        watch()
        expect(9, "zrepl:active")
  '';
}
