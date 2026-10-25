{ config, lib, pkgs, ... }:
# Agent-channel identities on tsugumi. See docs/agent-channel-design.md.
let
  # saya's client tester (machines/saya/agent-client.nix), once it's in the roster.
  client = lib.optional ((import ../../lib/agent-roster.nix).agents ? saya-client) "saya-client";
  freeResearch = import ../../lib/agent-free-research.nix;
  crashTrigger = "A Minecraft server crashed. Use the crash-analysis skill on the pending snapshot(s).";
  # Git credential helper handing the lab's GitHub PAT (Baughn, 2026-10-09: so it can
  # publish its Electrical Age port) to https://github.com. Only answers `get`.
  labGitCredential = pkgs.writeShellScript "lab-git-credential" ''
    [[ "$1" == get ]] || exit 0
    echo username=x-access-token
    echo "password=$(< ${config.age.secrets.agent-ea-pat.path})"
  '';

  labs = config.me.minecraft.labs;
  # Both lab agents' prompts: the role, then the lab's own names in lab-common.md.
  labPrompt = lab: role: pkgs.writeText "${lab.user}-prompt.md" (
    builtins.replaceStrings [ "@COMMON@" ] [ (builtins.replaceStrings
      [ "@UNIT@" "@MOUNTS@" "@CLONES@" "@QUOTA@" "@DAYS@" "@MEMORY@" "@CPUS@" "@LOGINPORT@"
        "@REMOTE@" "@OUTBOXDIR@" "@OUTBOXURL@" "@HANDOVER@" ]
      [ lab.unit lab.mounts (toString lab.maxClones) (lib.replaceStrings [ "G" ] [ " GB" ] lab.quota) (toString (lab.lifetime / 86400))
        (lib.replaceStrings [ "G" ] [ " GB" ] lab.memoryMax) (toString (lib.toInt (lib.removeSuffix "%" lab.cpuQuota) / 100))
        (toString lab.loginPort) lab.remoteLogin.address lab.outboxDir (labOutboxUrl lab) lab.handover ]
      (builtins.readFile ./agents/lab-common.md)) ] (builtins.readFile role));
  labOutboxUrl = lab: "http://10.171.0.1:${toString lab.outbox.port}/";

  # A lab agent's instance (minecraft-lab.nix): its lab's user, sandbox, directories
  # and units. tsugumi-lab works in labs.main, tsugumi-lab-assistant in labs.assistant.
  labInstance = lab: { tokenFile, role, askAgents, extraDirs ? [ ], allow ? [ ] }: {
    inherit tokenFile askAgents;
    user = lab.user;
    workdir = "/var/lib/${lab.user}/agent";
    channel = "main";
    promptFile = labPrompt lab role;
    board.socket = "/run/agent-board/api.sock";
    extraDirs = [ lab.mounts lab.handover lab.outboxDir "/srv/lab-git" ] ++ extraDirs;
    rcon.root = lab.mounts;
    # Any console command on its clones, without an approver or the classifier (Baughn,
    # msg 1558472455818055812: "Nothing there they couldn't do from the filesystem already").
    rcon.ask = [ ];
    # Long lab/client test loops need many agent round trips (Baughn, msg 1560278875378950235).
    limits.bot_streak = 60;
    path = [ config.system.build.minecraft-lab-client ];
    environment = {
      # git push to https://github.com/… authenticates with the PAT, no setup needed.
      GIT_CONFIG_COUNT = "1";
      GIT_CONFIG_KEY_0 = "credential.https://github.com.helper";
      GIT_CONFIG_VALUE_0 = "${labGitCredential}";
      # The lab's names, so shared tools needn't hardcode them (lab-common.md).
      MINECRAFT_LAB = lab.name;
      MINECRAFT_LAB_UNIT = lab.unit;
      MINECRAFT_LAB_DIR = lab.mounts;
      MINECRAFT_LAB_PORT = toString lab.loginPort;
      MINECRAFT_LAB_REMOTE = lab.remoteLogin.address;
      MINECRAFT_LAB_OUTBOX = lab.outboxDir;
      MINECRAFT_LAB_OUTBOX_URL = labOutboxUrl lab;
      MINECRAFT_LAB_HANDOVER = lab.handover;
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
      "Edit(//var/lib/${lab.user}/agent/**)"
      "Write(//var/lib/${lab.user}/agent/**)"
      "Edit(/${lab.mounts}/**)"
      "Write(/${lab.mounts}/**)"
      "Edit(/${lab.handover}/**)"
      "Write(/${lab.handover}/**)"
      "Edit(/${lab.outboxDir}/**)"
      "Write(/${lab.outboxDir}/**)"
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
      # Clones are disposable and the lab's slice caps every lab server together,
      # so the lab runs its own servers (Baughn, 2026-10-02). Polkit only lets the
      # lab user manage its own lab's @ units anyway.
      "Bash(systemctl start ${lab.unit}@*)"
      "Bash(systemctl stop ${lab.unit}@*)"
      "Bash(systemctl restart ${lab.unit}@*)"
      "Bash(minecraft-lab destroy *)"
      "mcp__bridge__rcon"
    ] ++ allow;
    ask = [
      "Bash(rm -r *)"
      "Bash(rm -rf *)"
    ];
    deny = [ "Bash(sudo *)" ];
    serviceConfig = lab.serviceConfig;
  };
in
{
  age.secrets = {
    agent-claude-token.file = ../../secrets/agent-claude-token.age;
    agent-tsugumi-minecraft-discord.file = ../../secrets/agent-tsugumi-minecraft-discord.age;
    agent-tsugumi-lab-discord.file = ../../secrets/agent-tsugumi-lab-discord.age;
    agent-tsugumi-lab-assistant-discord.file = ../../secrets/agent-tsugumi-lab-assistant-discord.age;
    # Both lab agents publish to GitHub with it.
    agent-ea-pat = {
      file = ../../secrets/agent-ea-pat.age;
      owner = labs.main.user;
      group = "lab-agents";
      mode = "0440";
    };
  };

  # Crash triggers, netcheck and the lab's status card go to the lab's agent.
  me.minecraft.labs.main.agent = "tsugumi-lab";
  me.minecraft.labs.assistant.agent = "tsugumi-lab-assistant";

  # Each lab agent lives in its lab's network namespace, beside its servers.
  systemd.services.agent-bridge-tsugumi-lab = {
    bindsTo = [ "${labs.main.unit}-pasta.service" ];
    after = [ "${labs.main.unit}-pasta.service" ];
  };
  systemd.services.agent-bridge-tsugumi-lab-assistant = {
    bindsTo = [ "${labs.assistant.unit}-pasta.service" ];
    after = [ "${labs.assistant.unit}-pasta.service" ];
  };

  # Shared bare git repos for the two lab agents (lab-common.md), and the PAT's group.
  # setgid keeps new repos in the group; git init --shared=group sets the modes,
  # since the bridges' umask is 0077.
  users.groups.lab-agents.members = [ labs.main.user labs.assistant.user ];

  # towers.brage.info (static-web.nix): the lab's browser build of the Stasis Delvers tower
  # generator, public (Baughn, msg 1559296753629663233). caddy-static reads it as "other",
  # so files must be world-readable; the bridge's umask makes them 0600.
  systemd.tmpfiles.rules = [
    "d /srv/towerview 0755 ${labs.main.user} ${labs.main.user} -"
    "d /srv/lab-git 2770 root lab-agents -"
    # Second-opinion dirs must exist for their ACLs.
    "d /var/lib/${labs.assistant.user}/agent/src 0700 ${labs.assistant.user} - -"
    "d /var/lib/${labs.assistant.user}/agent/notes 0700 ${labs.assistant.user} - -"
  ];

  # Gemini second opinions (modules/second-opinion.nix), with the paths each agent asked for
  # (msgs 1560358441791520832, 1560358537203425363).
  me.secondOpinion = {
    enable = true;
    agents.tsugumi-lab.dirs = map (d: "/var/lib/${labs.main.user}/agent/${d}") [
      "stasis" "notes/rifts" "notes/timestop" "notes/overnight" "eln/port26" "eln/notes"
    ];
    # Its clones and notes (tsugumi-lab-assistant.md says to keep them there).
    agents.tsugumi-lab-assistant.dirs = map (d: "/var/lib/${labs.assistant.user}/agent/${d}") [ "src" "notes" ];
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
      extraDirs = [ "/home/minecraft" ] ++ lib.mapAttrsToList (_: lab: lab.handover) labs;
      rcon.root = "/home/minecraft";
      # NixOS-level changes (units, scripts, its own permissions) go through saya.
      askAgents = [ "saya" "tsugumi-lab" "tsugumi-lab-assistant" ] ++ client;
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
    instances.tsugumi-lab = lib.recursiveUpdate (labInstance labs.main {
      tokenFile = config.age.secrets.agent-tsugumi-lab-discord.path;
      role = ./agents/tsugumi-lab.md;
      askAgents = [ "tsugumi-minecraft" "saya" "tsugumi-lab-assistant" ] ++ client;
      extraDirs = [ "/srv/towerview" ];
      allow = [ "Edit(//srv/towerview/**)" "Write(//srv/towerview/**)" ];
    }) {
      triggerSources.free-research = freeResearch.instruction;
      triggerTimers.free-research = freeResearch.calendar.tsugumi-lab;
      triggerThreads.free-research = freeResearch.thread;
      unattendedTriggers = [ "free-research" ];
    };

    # The second lab agent, for interruptions while tsugumi-lab does long work (Baughn,
    # msgs 1563885384859918410, 1563890365226950749). No free-research turns: the
    # alternate days are taken, and it exists to be free when something comes up.
    instances.tsugumi-lab-assistant = labInstance labs.assistant {
      tokenFile = config.age.secrets.agent-tsugumi-lab-assistant-discord.path;
      role = ./agents/tsugumi-lab-assistant.md;
      askAgents = [ "tsugumi-minecraft" "saya" "tsugumi-lab" ] ++ client;
    };
  };
}
