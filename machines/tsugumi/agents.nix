{ config, lib, pkgs, ... }:
# Agent-channel identities on tsugumi. See docs/agent-channel-design.md.
let
  # saya's client tester (machines/saya/agent-client.nix), once it's in the roster.
  client = lib.optional ((import ../../lib/agent-roster.nix).agents ? saya-client) "saya-client";
  freeResearch = import ../../lib/agent-free-research.nix;
  consolidation = import ../../lib/agent-consolidation.nix;
  crashTrigger = "A Minecraft server crashed. Use the crash-analysis skill on the pending snapshot(s).";
  # The merge queue's landings go to ErisiAgent and the result to the lab agent without
  # anyone polling (Baughn, msg 1573523617407570061). lab-merge writes the handover into
  # mergeHandover and the trigger files (format: the agent-channel triggerSources option).
  mergeHandover = "/srv/lab-handover-queue";
  handoverTrigger = "The lab's merge queue landed a change and wrote its handover; the note says where. "
    + "Check and stage it as you would any lab handover (checksum, README, built on what is live). "
    + "Passing the queue counts as Baughn's approval (msg 1573401987456831529); your usual rules for "
    + "restarts and players still apply. Say in the channel what you staged.";
  mergeResultTrigger = "Your merge queue job finished; the note has the outcome. If it landed, the queue "
    + "has handed it to ErisiAgent already: tell whoever was waiting, in their thread. If it was "
    + "rejected, find out why and fix or re-enqueue.";
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
  # The merge queue's runner (/srv/lab-git/queue/lab-merge, proc/merge-queue) as its own unit:
  # run from inside a bridge, a bridge restart (e.g. onto a deploy) killed it mid-gate
  # (LabAgent, msg 1573506711623438369). One instance per lab user, in that lab's netns,
  # started by `lab-merge run --detach`; lab-merge's own flock keeps it to one at a time.
  labMergeUnit = lab: {
    name = "lab-merge@${lab.user}";
    value = {
      description = "Lab merge queue runner (${lab.user})";
      bindsTo = [ "${lab.unit}-pasta.service" ];
      after = [ "${lab.unit}-pasta.service" ];
      path = with pkgs; [ git python3 bash coreutils findutils gnused gnutar gzip unzip which ];
      environment = {
        JDK25 = "${pkgs.jdk25}";
        JDK21 = "${pkgs.jdk21}";
        HOME = "/var/lib/${lab.user}";
        USER = lab.user;
        GIT_CONFIG_COUNT = "1";
        GIT_CONFIG_KEY_0 = "credential.https://github.com.helper";
        GIT_CONFIG_VALUE_0 = "${labGitCredential}";
      };
      serviceConfig = lab.serviceConfig // {
        Type = "oneshot";
        User = lab.user;
        Group = lab.user;
        SupplementaryGroups = [ "lab-agents" ];
        UMask = "0022";
        Slice = "lab-merge.slice";
        ExecStart = "/srv/lab-git/queue/lab-merge run";
      };
    };
  };
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
    extraDirs = [ lab.mounts lab.handover lab.outboxDir "/srv/lab-git" mergeHandover ] ++ extraDirs;
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
    triggerSources.merge-result = mergeResultTrigger;
    # Whichever lab runs the queue tells the submitter.
    triggerWriters.merge-result = [ labs.main.user labs.assistant.user ];
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
      "Bash(systemctl start lab-merge@${lab.user})"
      "Bash(systemctl stop lab-merge@${lab.user})"
      "mcp__bridge__rcon"
    ] ++ allow;
    ask = [
      "Bash(rm -r *)"
      "Bash(rm -rf *)"
    ];
    deny = [ "Bash(sudo *)" ];
    # Files they hand over must be readable by tsugumi-minecraft, caddy and the other
    # lab; the bridge's default 0077 kept tripping that (Baughn, msg 1571310395787059402).
    # Their home is 0700 and the bridge's state 0700, so nothing private opens up.
    serviceConfig = lab.serviceConfig // { UMask = "0022"; };
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
  systemd.services = {
    agent-bridge-tsugumi-lab = {
      bindsTo = [ "${labs.main.unit}-pasta.service" ];
      after = [ "${labs.main.unit}-pasta.service" ];
    };
    agent-bridge-tsugumi-lab-assistant = {
      bindsTo = [ "${labs.assistant.unit}-pasta.service" ];
      after = [ "${labs.assistant.unit}-pasta.service" ];
    };
  } // lib.listToAttrs (map labMergeUnit [ labs.main labs.assistant ]);

  # Both runners together (LabAgent: 16 GB, 8 CPUs).
  systemd.slices.lab-merge.sliceConfig = {
    MemoryMax = "16G";
    CPUQuota = "800%";
  };
  # Each lab user may start and stop only its own runner.
  security.polkit.extraConfig = lib.concatMapStrings (lab: ''
    polkit.addRule(function (action, subject) {
      if (action.id == "org.freedesktop.systemd1.manage-units" &&
          subject.user == "${lab.user}" &&
          action.lookup("unit") == "lab-merge@${lab.user}.service" &&
          ["start", "stop"].indexOf(action.lookup("verb")) >= 0) {
        return polkit.Result.YES;
      }
    });
  '') [ labs.main labs.assistant ];

  # Shared bare git repos for the two lab agents (lab-common.md), and the PAT's group.
  # setgid keeps new repos in the group; git init --shared=group sets the modes,
  # as other umasks may be stricter.
  users.groups.lab-agents.members = [ labs.main.user labs.assistant.user ];

  # towers.brage.info (static-web.nix): the lab's browser build of the Stasis Delvers tower
  # generator, public (Baughn, msg 1559296753629663233). caddy-static reads it as "other",
  # so files must be world-readable (the lab bridges' umask is 0022).
  systemd.tmpfiles.rules = [
    "d /srv/towerview 0755 ${labs.main.user} ${labs.main.user} -"
    "d /srv/lab-git 2770 root lab-agents -"
    # The queue's handovers: both lab users write, minecraft (ErisiAgent) reads.
    "d ${mergeHandover} 2770 root lab-agents -"
    "A+ ${mergeHandover} - - - - user:minecraft:r-x,default:user:minecraft:r-x"
    # Second-opinion dirs must exist for their ACLs.
    "d /var/lib/${labs.assistant.user}/agent/src 0700 ${labs.assistant.user} - -"
    "d /var/lib/${labs.assistant.user}/agent/notes 0700 ${labs.assistant.user} - -"
  ];

  # Gemini second opinions (modules/second-opinion.nix); Gemini reads what the asking agent can.
  me.secondOpinion = {
    enable = true;
    agents.tsugumi-lab = { };
    agents.tsugumi-lab-assistant = { };
    agents.tsugumi-minecraft.exclude = [ "/home/minecraft/agent/notes/handoff.md" "/home/minecraft/builder/tools/control/target" ];
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
      triggerSources.lab-handover = handoverTrigger;
      triggerWriters.lab-handover = [ labs.main.user labs.assistant.user ];
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
      allow = [
        "Edit(//srv/towerview/**)"
        "Write(//srv/towerview/**)"
        # The classifier refuses its GitHub pushes (EA develop) even after Baughn's OK in chat;
        # Baughn: "Add it to the whitelist" (msg 1570794768324370573).
        "Bash(git push origin *)"
      ];
    }) {
      triggerSources.free-research = freeResearch.instruction;
      triggerTimers.free-research = freeResearch.calendar.tsugumi-lab;
      triggerThreads.free-research = freeResearch.thread;
      triggerSources.consolidation = consolidation.instruction;
      triggerTimers.consolidation = consolidation.calendar;
      triggerThreads.consolidation = consolidation.thread;
      unattendedTriggers = [ "free-research" "consolidation" ];
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
