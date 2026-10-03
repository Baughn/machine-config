{ config, lib, pkgs, ... }:
# saya-client: runs a headless Minecraft client on saya's GPU against lab servers on
# tsugumi (me.minecraft.lab.remoteLogin), to test client-side fixes, and builds its own
# tooling for it (Baughn, msgs 1555923804000096337, 1555925717873594463).
#
# Inert until the bot is in lib/agent-roster.nix and Baughn has created its Discord token,
# secrets/agent-saya-client-discord.age (agenix -e). Its board token,
# secrets/agent-board-saya-client-token.age, is optional (tsugumi's agent-board-saya.nix
# reads it too).
let
  roster = import ../../lib/agent-roster.nix;
  user = "saya-client";
  home = "/home/saya-client";
  workdir = "${home}/agent";
  discordToken = ../../secrets/agent-saya-client-discord.age;
  boardToken = ../../secrets/agent-board-saya-client-token.age;
  ready = roster.agents ? ${user} && builtins.pathExists discordToken;
  hasBoard = builtins.pathExists boardToken;
in
lib.mkIf ready {
  age.secrets = {
    agent-saya-client-discord.file = discordToken;
  } // lib.optionalAttrs hasBoard {
    agent-board-saya-client-token.file = boardToken;
  };

  # Not in wheel: no sudo, not a trusted Nix user. video/render for the GPU's device nodes.
  users.users.${user} = {
    isSystemUser = true;
    group = user;
    extraGroups = [ "video" "render" ];
    inherit home;
    createHome = true;
    homeMode = "700";
  };
  users.groups.${user} = { };

  # Starting points from saya, read-only; the agent's own notes go in notes/ and tools/.
  systemd.tmpfiles.rules = [ "L+ ${workdir}/docs - - - - ${./agents/saya-client-docs}" ];

  me.agentChannel.instances.${user} = {
    inherit user workdir;
    channel = "main";
    tokenFile = config.age.secrets.agent-saya-client-discord.path;
    promptFile = ./agents/saya-client.md;
    board = lib.mkIf hasBoard {
      url = "http://10.171.0.1:8740";
      tokenFile = config.age.secrets.agent-board-saya-client-token.path;
    };
    # Admins may ask it for tests; it runs on Baughn's desktop, so only he approves.
    triggers = [ "owner" "admin" "agent" ];
    approvers = [ "owner" ];
    limits.bot_streak = 8;
    askAgents = [ "saya" "tsugumi-lab" "tsugumi-minecraft" ];
    permissionMode = "auto";
    model = "claude-opus-5-5";
    path = with pkgs; [
      jdk21 git curl unzip zip python3
      # Headless display on the GPU, screenshots, input, and checking what renders.
      cage gamescope xwayland xvfb xdotool grim mesa-demos vulkan-tools
    ];
    environment = {
      # /run/user is hidden (ProtectHome); a compositor needs somewhere for its socket.
      XDG_RUNTIME_DIR = "/run/agent-bridge-${user}";
      LAB_SERVER = "10.171.0.1:25666";
    };
    allow = [
      "Read"
      "Grep"
      "Glob"
      "WebSearch"
      "WebFetch"
      "Edit(/${workdir}/**)"
      "Write(/${workdir}/**)"
      "Bash(nix build *)"
      "Bash(nix eval *)"
      "Bash(java *)"
      "Bash(rg *)"
    ];
    deny = [ "Bash(sudo *)" ];
    serviceConfig = {
      NoNewPrivileges = true;
      # Its own home only; this also hides /run/user (Baughn's session, Wayland, xauth).
      ProtectHome = "tmpfs";
      BindPaths = [ home ];
      PrivateTmp = true;
      PrivateIPC = true;
      # The client, compositor and builds run in this unit's cgroup: keep the desktop usable.
      MemoryMax = "32G";
      CPUWeight = 50;
      IOWeight = 50;
    };
  };
}
