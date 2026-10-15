{ config, lib, pkgs, ... }:
# saya-printer (PrintAgent): controls and checks on Baughn's Prusa CORE One L over PrusaLink
# on the LAN, and watches it through the Buddy3D camera (Baughn, msg 1560396689464889436:
# "should _only_ be triggerable by me"). Owner-only: only Baughn triggers and approves it.
# It may read and pause freely; starting, resuming, stopping, uploading and deleting ask
# Baughn each time (his choice). It can slice with PrusaSlicer's stock profiles.
#
# Inert until the bot is in lib/agent-roster.nix and Baughn has created its Discord token,
# secrets/agent-saya-printer-discord.age, and the printer's credentials,
# secrets/prusa-printer.env.age (PRUSA_HOST, PRUSA_PASSWORD, PRUSA_USER, PRUSA_CAMERA).
let
  roster = import ../../lib/agent-roster.nix;
  user = "saya-printer";
  home = "/home/${user}";
  workdir = "${home}/agent";
  discordToken = ../../secrets/agent-saya-printer-discord.age;
  printerEnv = ../../secrets/prusa-printer.env.age;
  boardToken = ../../secrets/agent-board-saya-printer-token.age;
  ready = roster.agents ? ${user} && builtins.pathExists discordToken && builtins.pathExists printerEnv;
  hasBoard = builtins.pathExists boardToken;
  prusa = pkgs.writers.writePython3Bin "prusa" { flakeIgnore = [ "E501" ]; } (builtins.readFile ./prusa.py);
in
lib.mkIf ready {
  age.secrets = {
    agent-saya-printer-discord.file = discordToken;
    prusa-printer-env = {
      file = printerEnv;
      owner = user;
    };
  } // lib.optionalAttrs hasBoard {
    agent-board-saya-printer-token.file = boardToken;
  };

  # Not in wheel: no sudo, not a trusted Nix user.
  users.users.${user} = {
    isSystemUser = true;
    group = user;
    inherit home;
    createHome = true;
    homeMode = "700";
  };
  users.groups.${user} = { };

  me.agentChannel.instances.${user} = {
    inherit user workdir;
    channel = "main";
    tokenFile = config.age.secrets.agent-saya-printer-discord.path;
    promptFile = ./agents/saya-printer.md;
    board = lib.mkIf hasBoard {
      url = "http://10.171.0.1:8740";
      tokenFile = config.age.secrets.agent-board-saya-printer-token.path;
    };
    ownerOnly = true;
    triggers = [ "owner" ];
    approvers = [ "owner" ];
    # NixOS-level needs (a package, a permission) go through saya.
    askAgents = [ "saya" ];
    permissionMode = "auto";
    model = "claude-opus-5-5";
    path = [ prusa pkgs.ffmpeg-headless pkgs.prusa-slicer pkgs.curl pkgs.python3 pkgs.imagemagick ];
    environment = {
      PRUSA_ENV_FILE = config.age.secrets.prusa-printer-env.path;
      # PrusaSlicer wants somewhere to write its config.
      XDG_CONFIG_HOME = "${workdir}/.config";
    };
    allow = [
      "Read"
      "Grep"
      "Glob"
      "WebSearch"
      "WebFetch"
      "Edit(/${workdir}/**)"
      "Write(/${workdir}/**)"
      "Bash(prusa *)"
      "Bash(prusa-slicer *)"
    ];
    # Physical actions ask Baughn each time (ask beats allow). pause is the safe direction.
    ask = map (c: "Bash(prusa ${c}*)") [ "start" "resume" "stop" "upload" "delete" ];
    deny = [ "Bash(sudo *)" ];
    serviceConfig = {
      NoNewPrivileges = true;
      # Its own home only; this also hides /run/user (Baughn's session, Wayland, xauth).
      ProtectHome = "tmpfs";
      BindPaths = [ home ];
      PrivateTmp = true;
      PrivateIPC = true;
      MemoryMax = "8G";
      CPUWeight = 50;
    };
  };
}
