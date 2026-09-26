{ pkgs }:
let
  # Stands in for the builder's server/start.py: logs console lines, exits on
  # "stop" (0) or "crash" (1), and keeps running on EOF like a real server.
  # Like start.py's cleanup, it removes server.pid when it exits.
  fakeStart = pkgs.writeScript "fake-start.py" ''
    #!${pkgs.python3}/bin/python3
    import os, sys, time
    from pathlib import Path
    Path("server.pid").write_text(str(os.getpid()))
    Path("unit").write_text(os.environ.get("MINECRAFT_UNIT", ""))
    print(f"fake server {os.getpid()} up", flush=True)
    for line in sys.stdin:
        line = line.strip()
        with open("console.log", "a") as log:
            print(line, file=log)
        print(f"console: {line}", flush=True)
        if line in ("stop", "crash"):
            Path("server.pid").unlink()
            sys.exit(0 if line == "stop" else 1)
    while True:
        time.sleep(1)
  '';
  # Like the real one, found via PATH: instances' drop-ins once replaced the
  # template's PATH, and bash went missing.
  fakeUpdateAndStart = pkgs.writeScript "update-and-start.sh" ''
    #!/usr/bin/env bash
    exec /home/minecraft/w/server/start.py
  '';
  # Like the real control, it fails without server.pid.
  fakeControl = pkgs.writeShellScript "control.sh" ''
    cd /home/minecraft/w
    echo "$*" >> control.log
    pid=$(cat server.pid) || exit 1
    echo stop > /run/minecraft/w.stdin
    while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
  '';
in
pkgs.testers.runNixOSTest {
  name = "minecraft-servers";
  nodes.machine = { lib, ... }: {
    imports = [ ../machines/tsugumi/minecraft-servers.nix ];
    me.minecraft.autostart = [ "w" ];
    users.users.minecraft = { isNormalUser = true; uid = 1018; linger = true; };
    users.users.other = { isNormalUser = true; };
    systemd.services.bystander = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
    };
    systemd.tmpfiles.rules = [
      "d /home/minecraft/w 0755 minecraft users -"
      "d /home/minecraft/w/world 0755 minecraft users -"
      "d /home/minecraft/w/mods 0755 minecraft users -"
      "d /home/minecraft/w/server 0755 minecraft users -"
      "f /home/minecraft/w/server.nix-target 0644 minecraft users - w"
      "L+ /home/minecraft/w/server/start.py - - - - ${fakeStart}"
      "L+ /home/minecraft/w/update-and-start.sh - - - - ${fakeUpdateAndStart}"
      "L+ /home/minecraft/w/control.sh - - - - ${fakeControl}"
      # Not set up: no server.nix-target.
      "d /home/minecraft/bare 0755 minecraft users -"
      "d /home/minecraft/bare/world 0755 minecraft users -"
      "d /home/minecraft/bare/mods 0755 minecraft users -"
    ];
  };
  testScript = ''
    import shlex

    def as_user(user, command):
        return f"runuser -u {user} -- sh -c {shlex.quote(command)}"

    def pid():
        return machine.succeed("cat /home/minecraft/w/server.pid").strip()

    def wait_new_pid(before):
        machine.wait_until_succeeds(f"p=$(cat /home/minecraft/w/server.pid) && [ \"$p\" != {before} ]", timeout=60)

    def console_has(line):
        machine.wait_until_succeeds(f"grep -qx {shlex.quote(line)} /home/minecraft/w/console.log", timeout=30)

    machine.start()

    with subtest("autostart at boot, supervised, console FIFO owned by minecraft"):
        machine.wait_for_unit("minecraft@w.service")
        machine.wait_until_succeeds("test -s /home/minecraft/w/server.pid")
        assert machine.succeed("cat /home/minecraft/w/unit").strip() == "minecraft@w.service"
        assert machine.succeed("stat -c '%F %U %a' /run/minecraft/w.stdin").strip() == "fifo minecraft 600"
        unit = machine.succeed("systemctl cat minecraft@w.service")
        assert "X-RestartIfChanged=false" in unit, unit
        socket = machine.succeed("systemctl cat minecraft@w.socket")
        assert "X-RestartIfChanged=false" in socket, socket

    with subtest("console lines reach the server; other users can't write"):
        machine.succeed(as_user("minecraft", "echo hello > /run/minecraft/w.stdin"))
        console_has("hello")
        machine.fail(as_user("other", "echo intruder > /run/minecraft/w.stdin"))

    with subtest("minecraft can read its world's journal"):
        machine.wait_until_succeeds(as_user("minecraft", "journalctl -u minecraft@w.service -o cat | grep -q 'console: hello'"), timeout=30)

    with subtest("mc-console forwards input"):
        machine.succeed(as_user("minecraft", "echo via-console | timeout 10 mc-console w"))
        console_has("via-console")
        machine.fail(as_user("minecraft", "mc-console bare"))
        machine.fail(as_user("minecraft", "mc-console '../w'"))

    with subtest("a crash restarts the world; mc-console keeps working across it"):
        before = pid()
        machine.succeed(as_user("minecraft", f"""
            {{ echo crash
               timeout 60 sh -c 'until p=$(cat /home/minecraft/w/server.pid 2>/dev/null) && [ "$p" != {before} ]; do sleep 0.2; done'
               echo after-restart; }} | timeout 90 mc-console w
        """))
        console_has("after-restart")
        machine.wait_for_unit("minecraft@w.service")

    with subtest("a clean exit (daily restart, /stop) restarts the world"):
        before = pid()
        machine.succeed(as_user("minecraft", "echo stop > /run/minecraft/w.stdin"))
        wait_new_pid(before)
        machine.wait_for_unit("minecraft@w.service")
        # ExecStop has nothing to stop after a self-exit, so it mustn't run control.
        machine.fail("test -e /home/minecraft/w/control.log")

    with subtest("polkit: minecraft manages minecraft@ units and nothing else"):
        before = pid()
        machine.succeed(as_user("minecraft", "systemctl restart minecraft@w.service"))
        wait_new_pid(before)
        machine.succeed("grep -qx 'stop -t 10' /home/minecraft/w/control.log")
        machine.fail(as_user("minecraft", "systemctl stop bystander.service"))
        machine.fail(as_user("other", "systemctl stop minecraft@w.service"))
        machine.succeed("systemctl is-active bystander.service")

    with subtest("the guard refuses a world that is already running elsewhere"):
        machine.succeed(as_user("minecraft", "systemctl stop minecraft@w.service"))
        machine.fail("test -e /run/minecraft/w.stdin")
        # Stands in for a copy still running in tmux.
        machine.succeed("systemd-run --unit=tmux-standin --uid=minecraft --working-directory=/home/minecraft/w /home/minecraft/w/server/start.py")
        machine.wait_until_succeeds("test \"$(cat /home/minecraft/w/server.pid)\" = \"$(systemctl show -P MainPID tmux-standin)\"")
        machine.fail(as_user("minecraft", "systemctl start minecraft@w.service"))
        assert "already running" in machine.succeed("journalctl -u minecraft@w.service -o cat")
        machine.succeed("systemctl stop tmux-standin")
        machine.succeed("systemctl reset-failed minecraft@w.service")
        machine.succeed(as_user("minecraft", "systemctl start minecraft@w.service"))
        machine.wait_for_unit("minecraft@w.service")

    with subtest("directories that aren't set-up worlds are refused"):
        machine.fail("systemctl start minecraft@bare.service")
        machine.fail("systemctl start minecraft@nope.service")
  '';
}
