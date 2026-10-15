{ config, lib, pkgs, ... }:
# A second opinion from Gemini for the agents, over the Gemini API (Baughn, msgs
# 1560345194488926431 and 1560357948658819083; it replaces archive/agy-advisor). An opinion
# and pointers, never code. The service runs as its own user, which holds the API key: the
# agents only reach it through the socket, which tells it who is asking. Gemini gets Google
# Search, URL context and read-only file tools for the asking agent's directories, no shell.
# Each machine keeps its own monthly ledger; the AI Studio project's spend cap ($20) is the
# overall stop (Baughn's choice).
let
  cfg = config.me.secondOpinion;
  user = "second-opinion";
  state = "/var/lib/${user}";
  clients = "second-opinion-clients";
  # The unit sees each agent's directories here, bound read-only, so their parents (agent
  # workdirs that tmpfiles keeps at 0700, which masks any ACL) needn't be traversable.
  view = "/run/second-opinion-view";
  agents = lib.mapAttrs (id: a: a // { user = config.me.agentChannel.instances.${id}.user; }) cfg.agents;
  dirs = lib.unique (lib.concatMap (a: a.dirs) (lib.attrValues agents));
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
          dirs = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            description = "Directories Gemini may read for this agent; the first is the default.";
          };
          exclude = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = "Paths inside those directories to hide (private notes, build output).";
          };
        };
      });
      default = { };
      description = "Agent-channel instances (by id) that may ask, with what Gemini may read for each.";
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

    # Read access for the service's group; the default ACL covers files created later (if
    # their mode has group read).
    systemd.tmpfiles.rules = map (dir: "A+ ${dir} - - - - group:${user}:r-X,default:group:${user}:r-X") dirs;

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
        SO_MODEL = "gemini-4-argon";
        SO_CLIENTS = builtins.toJSON (lib.mapAttrs' (_: a: lib.nameValuePair a.user a.dirs) agents);
        SO_VIEW = view;
        SO_LEDGER = "${state}/ledger.json";
        SO_MONTHLY_USD = toString cfg.monthlyUsd;
        SO_CALL_USD = "1";
        # The agents' shell commands time out after 10 minutes.
        SO_TIMEOUT = "540";
      };
      path = [ pkgs.ripgrep ];
      serviceConfig = {
        ExecStart = "${lib.getExe script} serve";
        StandardInput = "socket";
        StandardOutput = "socket";
        StandardError = "journal";
        User = user;
        Group = user;
        StateDirectory = user;
        StateDirectoryMode = "0700";
        RuntimeMaxSec = "11min";
        TemporaryFileSystem = [ view ];
        # "-": a missing directory mustn't take the service down for everyone.
        BindReadOnlyPaths = map (dir: "-${dir}:${view}${dir}") dirs;
        InaccessiblePaths = map (p: "-${view}${p}") (lib.concatMap (a: a.exclude) (lib.attrValues agents));
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
      path = [ script ];
      allow = [ "Bash(second-opinion *)" ];
      skills.second-opinion = ./second-opinion/skill;
    }) cfg.agents;
  };
}
