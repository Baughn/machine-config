{ config, lib, ... }:
# Agent-channel identities on tsugumi. See docs/agent-channel-design.md.
let
  # saya's client tester (machines/saya/agent-client.nix), once it's in the roster.
  client = lib.optional ((import ../../lib/agent-roster.nix).agents ? saya-client) "saya-client";
  crashTrigger = "A Minecraft server crashed. Use the crash-analysis skill on the pending snapshot(s).";
in
{
  age.secrets = {
    agent-claude-token.file = ../../secrets/agent-claude-token.age;
    agent-tsugumi-minecraft-discord.file = ../../secrets/agent-tsugumi-minecraft-discord.age;
    agent-tsugumi-lab-discord.file = ../../secrets/agent-tsugumi-lab-discord.age;
  };

  # The lab agent lives in the lab's network namespace, beside its servers.
  systemd.services.agent-bridge-tsugumi-lab = {
    bindsTo = [ "minecraft-lab-pasta.service" ];
    after = [ "minecraft-lab-pasta.service" ];
  };

  me.agentChannel = {
    claudeTokenFile = config.age.secrets.agent-claude-token.path;

    # Auto mode in #auto-admin since 2026-09-26: the classifier approves most
    # calls, the ask rules below always reach a human, deny is refused.
    instances.tsugumi-minecraft = {
      user = "minecraft";
      workdir = "/home/minecraft/agent";
      channel = "main";
      tokenFile = config.age.secrets.agent-tsugumi-minecraft-discord.path;
      promptFile = ./agents/tsugumi-minecraft.md;
      board.socket = "/run/agent-board/api.sock";
      # The worlds sit beside the workdir; reading them shouldn't need an approver.
      extraDirs = [ "/home/minecraft" "/srv/lab-handover" ];
      rcon.root = "/home/minecraft";
      # NixOS-level changes (units, scripts, its own permissions) go through saya.
      askAgents = [ "saya" "tsugumi-lab" ] ++ client;
      # Baughn's tick-debugging workflow; its case records live in
      # /home/minecraft/agent-debugging, outside the skill.
      skills.minecraft-tick-debug = ./agents/skills/minecraft-tick-debug;
      skills.crash-analysis = ./agents/skills/crash-analysis;
      # crash_analysis.py (start.py, after an unrequested nonzero exit).
      triggerSources.crash-analysis = crashTrigger;
      permissionMode = "auto";
      # Pinned rather than the "opus" alias, so a model change is a deliberate
      # edit. Needs Claude Code >= 2.1.280, from nixpkgs-fast (see flake.nix).
      model = "claude-opus-5-5";
      allow = [
        "Read"
        "Grep"
        "Glob"
        "WebSearch"
        # Its own workdir.
        "Edit(//home/minecraft/agent/**)"
        "Write(//home/minecraft/agent/**)"
        "Bash(systemctl status *)"
        "Bash(systemctl show *)"
        "Bash(systemctl list-units *)"
        "Bash(journalctl *)"
        "Bash(zfs list *)"
        "Bash(zpool status *)"
        "Bash(ls *)"
        "Bash(cat *)"
        "Bash(head *)"
        "Bash(tail *)"
        "Bash(grep *)"
        "Bash(rg *)"
        "Bash(df *)"
        "Bash(free *)"
        "Bash(uptime)"
        "Bash(ps *)"
      ];
      # Command-text matches only, so easy to sidestep by accident; the backstops
      # are the snapshots and minecraft-watch (see "Accepted risk" in the doc).
      ask = [
        "Bash(systemctl start *)"
        "Bash(systemctl stop *)"
        "Bash(systemctl restart *)"
        "Bash(systemctl try-restart *)"
        "Bash(./control.sh stop *)"
        "Bash(./control.sh say *)"
        "Bash(rm -r *)"
        "Bash(rm -rf *)"
      ];
      deny = [ "Bash(sudo *)" ];
      # sudo can't elevate from inside the unit; see "Snapshot safety".
      serviceConfig.NoNewPrivileges = true;
    };

    # Experiments on ZFS clones of the worlds; see minecraft-lab.nix.
    instances.tsugumi-lab = let lab = config.me.minecraft.lab; in {
      user = lab.user;
      workdir = "/var/lib/${lab.user}/agent";
      channel = "main";
      tokenFile = config.age.secrets.agent-tsugumi-lab-discord.path;
      promptFile = ./agents/tsugumi-lab.md;
      board.socket = "/run/agent-board/api.sock";
      extraDirs = [ "/srv/minecraft-lab" "/srv/lab-handover" "/srv/lab-outbox" ];
      rcon.root = "/srv/minecraft-lab";
      # Serving the client tester needs no approver (Baughn, msg 1555957535054241905):
      # whitelist and op its offline account, and undo both. Added to the defaults.
      rcon.readOnly = lib.mkOptionDefault (lib.concatMap (name: [
        "whitelist add ${name}" "whitelist remove ${name}" "op ${name}" "deop ${name}"
      ]) [ "AdventurAgent" "adventuragent" ]);
      askAgents = [ "tsugumi-minecraft" "saya" ] ++ client;
      path = [ config.system.build.minecraft-lab-client ];
      # For testing crash analysis on clones whose lab.env enables it.
      skills.crash-analysis = ./agents/skills/crash-analysis;
      triggerSources.crash-analysis = crashTrigger;
      permissionMode = "auto";
      model = "claude-opus-5-5";
      allow = [
        "Read"
        "Grep"
        "Glob"
        "WebSearch"
        "Edit(//var/lib/mclab/agent/**)"
        "Write(//var/lib/mclab/agent/**)"
        "Edit(//srv/minecraft-lab/**)"
        "Write(//srv/minecraft-lab/**)"
        "Edit(//srv/lab-handover/**)"
        "Write(//srv/lab-handover/**)"
        "Edit(//srv/lab-outbox/**)"
        "Write(//srv/lab-outbox/**)"
        "Bash(minecraft-lab list)"
        "Bash(systemctl status *)"
        "Bash(systemctl show *)"
        "Bash(journalctl *)"
        "Bash(ls *)"
        "Bash(cat *)"
        "Bash(head *)"
        "Bash(tail *)"
        "Bash(grep *)"
        "Bash(rg *)"
        "Bash(df *)"
        "Bash(free *)"
        "Bash(ps *)"
        # Clones are disposable and mclab.slice caps every lab server together
        # at 16G, so the lab runs its own servers (Baughn, 2026-10-02). Polkit
        # only lets mclab manage minecraft-lab@ units anyway.
        "Bash(systemctl start minecraft-lab@*)"
        "Bash(systemctl stop minecraft-lab@*)"
        "Bash(systemctl restart minecraft-lab@*)"
        "Bash(minecraft-lab destroy *)"
      ];
      ask = [
        "Bash(rm -r *)"
        "Bash(rm -rf *)"
      ];
      deny = [ "Bash(sudo *)" ];
      serviceConfig = lab.serviceConfig;
    };
  };
}
