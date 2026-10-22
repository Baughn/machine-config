{ config, lib, pkgs, flakeSelf, dessplay, ... }:
# tsugumi-sec: a security watch over tsugumi's network-facing code (Baughn, Discord thread
# 1558438110084341874; plan on board thread #13). Every four hours a root oneshot collects what
# is exposed and what runs behind it, and starts the agent's turn: the first run after 05:00 is
# the full daily review, with vulnix; the others are change checks (Baughn, msg 1562834074505056381).
#
# It reads hostile text all day, so it is fenced in on the assumption that it gets injected:
# no shell, no permission prompts (dontAsk: unlisted calls are refused), read access only to its
# own files, the inventory and /nix/store; writes only to its notes and sources directory; no
# LAN or localhost; its Discord messages never trigger other agents (contextOnly in the roster)
# and its board threads are private to it. It asks for more in its reports.
#
# Inert until the bot is in lib/agent-roster.nix and Baughn has created its Discord token,
# secrets/agent-tsugumi-security-discord.age (agenix -e).
let
  roster = import ../../lib/agent-roster.nix;
  id = "tsugumi-sec";
  user = id;
  home = "/var/lib/${user}";
  workdir = "${home}/agent";
  sourcesDir = "${home}/sources";
  inventory = "/var/lib/${id}-inventory";
  # The deployed nixpkgs, already in the closure through nix.nixPath (modules/nix.nix).
  nixpkgs = "${flakeSelf.inputs.nixpkgs}";
  discordToken = ../../secrets/agent-tsugumi-security-discord.age;
  ready = roster.agents ? ${id} && builtins.pathExists discordToken;

  # What the config exposes, as built: the agent compares this with what actually listens.
  fw = config.networking.firewall;
  exposed = pkgs.writeText "${id}-exposed.json" (builtins.toJSON {
    firewall = {
      inherit (fw) allowedTCPPorts allowedUDPPorts allowedTCPPortRanges allowedUDPPortRanges;
      interfaces = lib.mapAttrs (_: v: { inherit (v) allowedTCPPorts allowedUDPPorts; }) fw.interfaces;
    };
    caddy = {
      # Most sites live in extraConfig, not virtualHosts.
      inherit (config.services.caddy) globalConfig extraConfig;
      virtualHosts = lib.mapAttrs (_: v: { inherit (v) serverAliases extraConfig; }) config.services.caddy.virtualHosts;
    };
    openssh = {
      inherit (config.services.openssh) ports;
      settings = lib.filterAttrs (_: v: v != null) config.services.openssh.settings;
    };
    wireguard = lib.mapAttrs (_: v: { inherit (v) listenPort ips; }) config.networking.wireguard.interfaces;
    minecraftWorlds = config.me.minecraft.autostart;
    note = "punch (modules/punch.nix) opens game ports per source IP at runtime, after Discord authorization.";
  });

  # vulnix gives each NVD feed download 10 s, which the NVD doesn't always make (16 Oct);
  # Baughn asked for a longer timeout (msg 1560596928151031839). The inventory retries too.
  vulnix = pkgs.vulnix.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace src/vulnix/nvd.py --replace-fail "timeout=10)" "timeout=120)"
    '';
  });

  inventoryScript = pkgs.writers.writePython3 "${id}-inventory" { flakeIgnore = [ "E501" ]; }
    (builtins.readFile ./agent-sec-inventory.py);
  # The home ISP's whole range (Baughn: "deny the entire ISP"): 2001:bb6::/32 (NGA/eir, what
  # tsugumi and saya have now) and 2a02:8080::/29 (Virgin Media Ireland, the older prefix in lan.nix).
  denied = [
    "localhost" "link-local" "multicast"
    "10.0.0.0/8" "172.16.0.0/12" "192.168.0.0/16" "100.64.0.0/10" "fc00::/7"
    "2001:bb6::/32" "2a02:8080::/29"
  ];
in
lib.mkIf ready {
  age.secrets.agent-tsugumi-security-discord.file = discordToken;

  users.users.${user} = {
    isSystemUser = true;
    group = user;
    inherit home;
    createHome = true;
    homeMode = "700";
  };
  users.groups.${user} = { };

  me.agentBoard.users.${user} = id;
  me.agentBoard.privateAgents = [ id ];

  systemd.tmpfiles.rules = [
    "d ${workdir}/notes 0700 ${user} ${user} -"
    "d ${sourcesDir} 0700 ${user} ${user} -"
    "d ${inventory} 0750 root ${user} -"
    # Attachments people send it (Garibaldi's setup request; Baughn approved, msg
    # 1562496082364342414): the bridge's inbox, bind-mounted read-only into the workdir,
    # because the rest of the bridge's state (the CLI's home) stays denied.
    "d /var/lib/agent-bridge/${id}/inbox 0700 ${user} ${user} -"
    "d ${workdir}/inbox 0700 ${user} ${user} -"
  ];

  me.agentChannel.instances.${id} = {
    inherit user workdir;
    channel = "main";
    tokenFile = config.age.secrets.agent-tsugumi-security-discord.path;
    promptFile = ./agents/tsugumi-sec.md;
    board.socket = "/run/agent-board/api.sock";
    # Baughn talks to it; nothing else starts its turns but the inventory timer.
    ownerOnly = true;
    triggers = [ "owner" ];
    approvers = [ "owner" ];
    triggerSources.daily = ''
      The daily inventory is ready (paths in the note). Do your daily security review as your
      prompt describes, and write it up.
    '';
    triggerSources.check = ''
      A fresh inventory is ready (paths in the note). Do your four-hourly check as your prompt
      describes.
    '';
    builtinTools = false;
    # Its home is read-only (ProtectSystem below); the CLI and nix get the state directory.
    environment.HOME = "/var/lib/agent-bridge/${id}";
    sources = {
      dir = sourcesDir;
      maxBytes = 3 * 1024 * 1024 * 1024;
      nixpkgs = nixpkgs;
    };
    extraDirs = [ inventory sourcesDir ];
    # A fresh session for most runs (every 4 h): whatever it read earlier doesn't ride along.
    idleReset = 2 * 3600;
    limits = { posts_per_minute = 3; posts_per_hour = 12; turns_per_hour = 6; };
    permissionMode = "dontAsk";
    model = "claude-opus-5-5";
    allow = [
      "Read(/${workdir}/**)"
      "Read(/${inventory}/**)"
      "Read(/${sourcesDir}/**)"
      "Read(//nix/store/**)"
      "Grep(/${workdir}/**)"
      "Grep(/${inventory}/**)"
      "Grep(/${sourcesDir}/**)"
      "Grep(//nix/store/**)"
      "Glob(/${workdir}/**)"
      "Glob(/${inventory}/**)"
      "Glob(/${sourcesDir}/**)"
      "Glob(//nix/store/**)"
      "Edit(/${workdir}/notes/**)"
      "Write(/${workdir}/notes/**)"
      "WebSearch"
      "WebFetch"
    ];
    # Deny wins over allow and over the CLI's own read-only defaults.
    deny = [
      # schedule is allowed (Baughn, msg 1559353262581489697): limits.turns_per_hour still caps it.
      "Bash" "NotebookEdit" "AskUserQuestion"
      "Read(//run/**)" "Read(//proc/**)" "Read(//var/lib/agent-bridge/**)" "Read(//etc/**)"
      "Grep(//run/**)" "Grep(//proc/**)" "Grep(//var/lib/agent-bridge/**)"
      "Glob(//run/**)" "Glob(//proc/**)" "Glob(//var/lib/agent-bridge/**)"
      "Edit(/${workdir}/.claude/**)" "Write(/${workdir}/.claude/**)"
      "Edit(/${workdir}/CLAUDE.md)" "Write(/${workdir}/CLAUDE.md)"
    ];
    serviceConfig = {
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      # Writable: its notes and sources, and the bridge's state (StateDirectory). The rest of
      # the workdir, .claude/ included, is read-only, so it can't hot-load settings or hooks.
      ReadWritePaths = [ "${workdir}/notes" sourcesDir ];
      ReadOnlyPaths = [ "-${inventory}" ];
      BindReadOnlyPaths = [ "/var/lib/agent-bridge/${id}/inbox:${workdir}/inbox" ];
      PrivateTmp = true;
      PrivateDevices = true;
      PrivateIPC = true;
      ProtectKernelTunables = true;
      ProtectKernelModules = true;
      ProtectControlGroups = true;
      ProtectProc = "invisible";
      RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
      RestrictNamespaces = true;
      LockPersonality = true;
      IPAddressDeny = denied;
      # resolved's stub; nss-resolve talks varlink anyway.
      IPAddressAllow = [ "127.0.0.53" ];
      MemoryMax = "8G";
      CPUQuota = "200%";
    };
  };

  systemd.services."${id}-inventory" = {
    description = "Inventory for the security watch (${id})";
    path = [ pkgs.iproute2 vulnix config.nix.package config.systemd.package ];
    environment = {
      VULNIX_CACHE = "/var/cache/${id}-vulnix";
      VULNIX_WHITELIST = "${./agent-sec-vulnix-whitelist.toml}";
      MC_ROOT = "/home/minecraft";
      MC_USER = "minecraft";
      MC_WORLDS = lib.concatStringsSep " " config.me.minecraft.autostart;
      NIXPKGS_PATH = nixpkgs;
      NIXPKGS_VERSION = lib.version;
      # What this system was built from (Garibaldi's setup request; Baughn approved, msg
      # 1559353262581489697). The system already depends on the rev (modules/nix-deploy.nix).
      MACHINE_CONFIG_REV = toString config.system.configurationRevision;
      FLAKE_INPUTS = builtins.toJSON (lib.mapAttrs (_: input: input.rev or null) flakeSelf.inputs);
      BUILT = builtins.toJSON { dessplay = "${dessplay.packages.${pkgs.stdenv.hostPlatform.system}.default}"; };
      # The store is read-only here; root would otherwise open the database itself.
      NIX_REMOTE = "daemon";
    };
    serviceConfig = {
      Type = "oneshot";
      ExecStart = lib.escapeShellArgs [
        inventoryScript inventory exposed "/var/lib/agent-bridge/${id}/triggers" user
      ];
      StateDirectory = "${id}-inventory";
      StateDirectoryMode = "0750";
      CacheDirectory = "${id}-vulnix";
      Group = user;
      UMask = "0027";
      # Root for ss -p, /proc/<pid>/{exe,cmdline,cwd}, the mods directories and the agent's trigger directory.
      CapabilityBoundingSet = [ "CAP_DAC_READ_SEARCH" "CAP_DAC_OVERRIDE" "CAP_SYS_PTRACE" "CAP_NET_ADMIN" "CAP_CHOWN" "CAP_FOWNER" ];
      ProtectSystem = "strict";
      ProtectHome = "read-only";
      ReadWritePaths = [ "/var/lib/agent-bridge/${id}/triggers" ];
      PrivateTmp = true;
      PrivateDevices = true;
      NoNewPrivileges = true;
      # vulnix may run three times (two retries).
      TimeoutStartSec = "4h";
    };
  };

  systemd.timers."${id}-inventory" = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnCalendar = "*-*-* 01,05,09,13,17,21:30:00";
      RandomizedDelaySec = "15min";
      Persistent = true;
    };
  };
}
