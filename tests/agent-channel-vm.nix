{ pkgs }:
# The OS-level boundary around a bridge unit. The bridge runs in fake mode
# (no Discord, no Claude), since the VM has no network.
pkgs.testers.runNixOSTest {
  name = "agent-channel";
  nodes.machine = { ... }: {
    imports = [ ../modules/agent-channel.nix ];
    users.users.agent = { isNormalUser = true; extraGroups = [ "wheel" ]; };
    users.users.other = { isNormalUser = true; };
    security.sudo.wheelNeedsPassword = false;
    me.agentChannel = {
      claudeTokenFile = toString (pkgs.writeText "claude-token" "claude-secret");
      instances.tsugumi-minecraft = {
        user = "agent";
        workdir = "/home/agent/agent";
        tokenFile = toString (pkgs.writeText "discord-token" "discord-secret");
        permissionMode = "auto";
        deny = [ "Bash(sudo *)" ];
        serviceConfig.NoNewPrivileges = true;
        fake = true;
      };
    };
  };
  testScript = ''
    machine.wait_for_unit("agent-bridge-tsugumi-minecraft.service")
    unit = "agent-bridge-tsugumi-minecraft.service"
    pid = machine.succeed(f"systemctl show -P MainPID {unit}").strip()

    with subtest("the bridge runs as its user, with the rendered config accepted"):
        assert machine.succeed(f"ps -o user= -p {pid}").strip() == "agent"
        machine.wait_until_succeeds(f"journalctl -u {unit} | grep -q 'fake mode'", timeout=30)
        machine.succeed("test \"$(stat -c %U:%a /home/agent/agent)\" = agent:700")

    with subtest("no privilege escalation from inside the unit"):
        machine.succeed(f"grep -q '^NoNewPrivs:\\s*1' /proc/{pid}/status")
        # The same user outside the unit can sudo; that is the lingering-manager
        # caveat in the design, not something the unit prevents.
        machine.succeed("runuser -u agent -- sudo -n true")
        machine.fail("systemd-run --wait --pipe --uid=agent -p NoNewPrivileges=yes sudo -n true")

    with subtest("credentials and state are private"):
        creds = f"/run/credentials/{unit}"
        assert machine.succeed(f"cat {creds}/discord-token").strip() == "discord-secret"
        machine.succeed(f"runuser -u agent -- test -r {creds}/claude-token")
        machine.fail(f"runuser -u other -- cat {creds}/claude-token")
        machine.succeed("test \"$(stat -c %U:%a /var/lib/agent-bridge/tsugumi-minecraft)\" = agent:700")
        machine.succeed("runuser -u agent -- touch /var/lib/agent-bridge/tsugumi-minecraft/session.json")
        machine.fail("runuser -u other -- ls /var/lib/agent-bridge/tsugumi-minecraft")

    with subtest("a workdir that sets permissions is refused"):
        machine.succeed("install -d -o agent /home/agent/agent/.claude",
                        "echo '{\"permissions\": {\"allow\": [\"Bash(*)\"]}}' > /home/agent/agent/.claude/settings.json")
        machine.succeed(f"systemctl restart {unit} || true")
        machine.wait_until_succeeds(f"journalctl -u {unit} | grep -q 'sets permissions'")
        machine.fail(f"systemctl is-active {unit}")
  '';
}
