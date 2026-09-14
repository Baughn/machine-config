{ pkgs }:
pkgs.testers.runNixOSTest {
  name = "minecraft-shutdown";
  nodes.machine = { lib, ... }: {
    imports = [ ../machines/tsugumi/minecraft-shutdown.nix ];
    users.users.minecraft = {
      isNormalUser = true;
      uid = 1018;
      linger = true;
    };
    # Exercise the production failure mode quickly with a deliberately stuck hook.
    systemd.services.minecraft-shutdown.serviceConfig.TimeoutStopSec = lib.mkForce "2s";
    systemd.tmpfiles.rules = [
      "d /home/minecraft/builder 0755 minecraft users -"
    ];
    environment.etc."test-minecraft-shutdown.py".text = ''
      import os
      from pathlib import Path
      import signal
      import subprocess
      import time

      assert os.getuid() == 1018
      assert os.environ["XDG_RUNTIME_DIR"] == "/run/user/1018"
      subprocess.run(["${pkgs.systemd}/bin/systemctl", "--user", "is-system-running"], check=True)
      Path("/home/minecraft/hook-ran").write_text("user manager was still running")
      if Path("/home/minecraft/hang").exists():
          signal.signal(signal.SIGTERM, signal.SIG_IGN)
          # A child that ignores SIGTERM must also die at the same deadline.
          subprocess.Popen(["${pkgs.python3}/bin/python3", "-c",
              "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(300)"])
          time.sleep(300)
    '';
  };
  testScript = ''
    import time

    machine.start()
    machine.wait_for_unit("minecraft-shutdown.service")
    machine.wait_for_unit("user@1018.service")
    machine.succeed("cp /etc/test-minecraft-shutdown.py /home/minecraft/builder/shutdown.py")
    machine.fail("test -e /home/minecraft/hook-ran")
    unit = machine.succeed("systemctl cat minecraft-shutdown.service")
    assert "X-RestartIfChanged=false" in unit
    assert "X-StopIfChanged=false" in unit
    machine.succeed("systemctl daemon-reload; systemctl start minecraft-shutdown.service")
    machine.fail("test -e /home/minecraft/hook-ran")

    # A real reboot must invoke the hook before the user manager stops.
    machine.succeed("systemctl reboot")
    machine.wait_for_shutdown()
    machine.start()
    machine.wait_for_unit("minecraft-shutdown.service")
    assert "user manager was still running" in machine.succeed("cat /home/minecraft/hook-ran")

    # A stuck hook must not consume a second timeout waiting for SIGTERM.
    machine.succeed("touch /home/minecraft/hang")
    before = time.monotonic()
    machine.execute("systemctl stop minecraft-shutdown.service")
    assert time.monotonic() - before < 4, "shutdown hook exceeded its single timeout"
    assert "timeout" in machine.succeed("systemctl show minecraft-shutdown.service -p Result --value")
    machine.fail("pgrep -f 'time.sleep\\(300\\)'")
    machine.succeed("systemctl --user --machine=minecraft@.host is-system-running")
  '';
}
