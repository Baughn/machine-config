{ config, lib, pkgs, ... }:
# A second opinion from Gemini for the agents, over the Gemini API (Baughn, msgs
# 1560345194488926431 and 1560357948658819083; it replaces archive/agy-advisor). An opinion
# and pointers, never code. The service runs as its own user, which holds the API key: the
# agents only reach it through the socket, which tells it who is asking. Gemini gets Google
# Search, URL context and read-only file tools, no shell. The file tools run in the agent's
# own client process, so Gemini reads what the agent can (Baughn, msg 1570789306484920362),
# minus secrets and each agent's `exclude`; the service reads no agent files itself.
# Each machine keeps its own monthly ledger; the AI Studio project's spend cap ($20) is the
# overall stop (Baughn's choice).
let
  cfg = config.me.secondOpinion;
  user = "second-opinion";
  state = "/var/lib/${user}";
  clients = "second-opinion-clients";
  agents = lib.mapAttrs (id: a: a // { user = config.me.agentChannel.instances.${id}.user; }) cfg.agents;
  script = pkgs.writers.writePython3Bin "second-opinion"
    { libraries = [ pkgs.python3Packages.google-genai ]; flakeIgnore = [ "E501" ]; }
    (builtins.readFile ./second-opinion/second-opinion.py);
in
{
  options.me.secondOpinion = {
    enable = lib.mkEnableOption "the second-opinion service (needs secrets/gemini-api-key.age for this machine)";
    monthlyUsd = lib.mkOption {
      type = lib.types.ints.positive;
      default = 20;
      description = "This machine's monthly budget; calls past it are refused.";
    };
    agents = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options = {
          exclude = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = "Paths to hide from Gemini (private notes, build output); secrets are always hidden.";
          };
        };
      });
      default = { };
      description = "Agent-channel instances (by id) that may ask, with what to hide from Gemini for each.";
    };
  };

  config = lib.mkIf cfg.enable {
    age.secrets.gemini-api-key = {
      file = ../secrets/gemini-api-key.age;
      owner = user;
    };

    users.users.${user} = {
      isSystemUser = true;
      group = user;
      home = state;
    };
    users.groups.${user} = { };
    users.groups.${clients}.members = lib.unique (lib.mapAttrsToList (_: a: a.user) agents);

    systemd.sockets.second-opinion = {
      description = "Second opinions from Gemini for the agents";
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = "/run/second-opinion/socket";
        SocketGroup = clients;
        SocketMode = "0660";
        Accept = true;
        MaxConnections = 3;
      };
    };

    systemd.services."second-opinion@" = {
      description = "A second opinion from Gemini";
      environment = {
        GEMINI_API_KEY_FILE = config.age.secrets.gemini-api-key.path;
        # Baughn, msg 1570617277282386031: Carbon on flex (half price; queued, so rounds fall
        # back to standard when flex is too slow or busy to answer in time).
        SO_MODEL = "gemini-4-carbon";
        SO_TIER = "flex";
        SO_CLIENTS = builtins.toJSON (lib.mapAttrs' (_: a: lib.nameValuePair a.user a.exclude) agents);
        SO_LEDGER = "${state}/ledger.json";
        SO_USAGE = "${state}/usage.jsonl";
        SO_CONVERSATIONS = "${state}/conversations";
        SO_MONTHLY_USD = toString cfg.monthlyUsd;
        SO_CALL_USD = "1";
        # The agents' Bash calls may run 20 minutes (BASH_MAX_TIMEOUT_MS in the bridge; Baughn,
        # msg 1570619532744196169): Carbon on flex can be slow.
        SO_TIMEOUT = "1140";
      };
      serviceConfig = {
        ExecStart = "${lib.getExe script} serve";
        StandardInput = "socket";
        StandardOutput = "socket";
        StandardError = "journal";
        User = user;
        Group = user;
        StateDirectory = user;
        StateDirectoryMode = "0700";
        RuntimeMaxSec = "21min";
        ProtectSystem = "strict";
        ProtectHome = "tmpfs";
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
        SystemCallFilter = [ "@system-service" ];
      };
    };

    me.agentChannel.instances = lib.mapAttrs (_: _: {
      path = [ script pkgs.ripgrep ];
      allow = [ "Bash(second-opinion *)" ];
      skills.second-opinion = ./second-opinion/skill;
    }) cfg.agents;
  };
}
