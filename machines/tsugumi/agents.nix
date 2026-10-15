{ config, lib, pkgs, ... }:
# Agent-channel identities on tsugumi. See docs/agent-channel-design.md.
let
  # saya's client tester (machines/saya/agent-client.nix), once it's in the roster.
  client = lib.optional ((import ../../lib/agent-roster.nix).agents ? saya-client) "saya-client";
  crashTrigger = "A Minecraft server crashed. Use the crash-analysis skill on the pending snapshot(s).";
  # Git credential helper handing the lab's GitHub PAT (Baughn, 2026-10-09: so it can
  # publish its Electrical Age port) to https://github.com. Only answers `get`.
  labGitCredential = pkgs.writeShellScript "lab-git-credential" ''
    [[ "$1" == get ]] || exit 0
    echo username=x-access-token
    echo "password=$(< ${config.age.secrets.agent-ea-pat.path})"
  '';
in
{
  age.secrets = {
    agent-claude-token.file = ../../secrets/agent-claude-token.age;
    agent-tsugumi-minecraft-discord.file = ../../secrets/agent-tsugumi-minecraft-discord.age;
    agent-tsugumi-lab-discord.file = ../../secrets/agent-tsugumi-lab-discord.age;
    agent-ea-pat = {
      file = ../../secrets/agent-ea-pat.age;
      owner = config.me.minecraft.lab.user;
      mode = "0400";
    };
  };

  # The lab agent lives in the lab's network namespace, beside its servers.
  systemd.services.agent-bridge-tsugumi-lab = {
    bindsTo = [ "minecraft-lab-pasta.service" ];
    after = [ "minecraft-lab-pasta.service" ];
  };

  # towers.brage.info (static-web.nix): the lab's browser build of the Stasis Delvers tower
  # generator, public (Baughn, msg 1559296753629663233). caddy-static reads it as "other",
  # so files must be world-readable; the bridge's umask makes them 0600.
  systemd.tmpfiles.rules = [ "d /srv/towerview 0755 ${config.me.minecraft.lab.user} ${config.me.minecraft.lab.user} -" ];

  # Gemini second opinions (modules/second-opinion.nix), with the paths each agent asked for
  # (msgs 1560358441791520832, 1560358537203425363). Off until secrets/gemini-api-key.age is
  # rekeyed for tsugumi.
  me.secondOpinion = {
    enable = false;
    agents.tsugumi-lab.dirs = map (d: "/var/lib/${config.me.minecraft.lab.user}/agent/${d}") [
      "stasis" "notes/rifts" "notes/timestop" "notes/overnight" "eln/port26" "eln/notes"
    ];
    agents.tsugumi-minecraft = {
      dirs = [ "/home/minecraft/builder" "/home/minecraft/agent/notes" "/home/minecraft/agent/tools" ];
      exclude = [ "/home/minecraft/agent/notes/handoff.md" "/home/minecraft/builder/tools/control/target" ];
    };
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
      # Long lab/client test loops need many agent round trips (Baughn, msg 1560278875378950235).
      limits.bot_streak = 60;
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
        # A false restart costs little and it hasn't asked for a wrong one yet (Baughn,
        # msg 1560062968354447361). Polkit lets it touch only minecraft@ units anyway.
        "Bash(systemctl restart minecraft@*)"
      ];
      # Command-text matches only, so easy to sidestep by accident; the backstops
      # are the snapshots and minecraft-watch (see "Accepted risk" in the doc).
      ask = [
        "Bash(systemctl start *)"
        "Bash(systemctl stop *)"
        # No "systemctl restart *" here: ask beats allow, so it would override the
        # allow above. Other restarts go to the classifier, and polkit refuses them.
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
      extraDirs = [ "/srv/minecraft-lab" "/srv/lab-handover" "/srv/lab-outbox" "/srv/towerview" ];
      rcon.root = "/srv/minecraft-lab";
      # Any console command on its clones, without an approver or the classifier (Baughn,
      # msg 1558472455818055812: "Nothing there they couldn't do from the filesystem already").
      rcon.ask = [ ];
      askAgents = [ "tsugumi-minecraft" "saya" ] ++ client;
      # Long lab/client test loops need many agent round trips (Baughn, msg 1560278875378950235).
      limits.bot_streak = 60;
      path = [ config.system.build.minecraft-lab-client ];
      # git push to https://github.com/… authenticates with the PAT, no setup needed.
      environment = {
        GIT_CONFIG_COUNT = "1";
        GIT_CONFIG_KEY_0 = "credential.https://github.com.helper";
        GIT_CONFIG_VALUE_0 = "${labGitCredential}";
      };
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
        "Edit(//srv/towerview/**)"
        "Write(//srv/towerview/**)"
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
        "mcp__bridge__rcon"
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
