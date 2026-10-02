{ config, lib, pkgs, ... }:

# The agent board (tools/agent-board): threads per work topic, summaries with revisions,
# full-text search. The agents reach the API on a unix socket that identifies them by uid
# (the lab's network fence lets unix sockets through), saya over wg0 with a bearer token,
# and Baughn reads the HTML at agents.brage.info (caddy.nix). Plan: msg 1554845772414189569.
let
  cfg = config.me.agentBoard;
  board = pkgs.callPackage ../../tools/agent-board { };
  stateDir = "/var/lib/agent-board";
  configFile = pkgs.writeText "agent-board.json" (builtins.toJSON {
    inherit (cfg) users;
    tokens = lib.mapAttrs (agent: _: "token-${agent}") cfg.tokens;
  });
  # The sockets outlive service restarts, so callers queue instead of failing.
  socketDefaults = {
    wantedBy = [ "sockets.target" ];
  };
in
{
  options.me.agentBoard = {
    users = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Unix users allowed on the API socket, and the agent id each posts as.";
    };
    tokens = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      description = "Agent id to a file holding its bearer token, for the TCP listener.";
    };
    httpAddress = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "10.171.0.1:8740";
      description = "TCP address for token-authenticated API callers; off when null.";
    };
    htmlGroup = lib.mkOption {
      type = lib.types.str;
      default = config.services.caddy.group;
      description = "Group allowed on the HTML socket (the reverse proxy).";
    };
  };

  config = {
    me.agentBoard.users = {
      minecraft = "tsugumi-minecraft";
      mclab = "tsugumi-lab";
    };

    assertions = [{
      assertion = (cfg.httpAddress != null) == (cfg.tokens != { });
      message = "me.agentBoard: httpAddress and tokens go together.";
    }];

    users.users.agent-board = {
      isSystemUser = true;
      group = "agent-board";
      home = stateDir;
    };
    users.groups.agent-board = { };

    # Mode 0666 is deliberate: the service answers only uids listed in `users`.
    systemd.sockets.agent-board-api = socketDefaults // {
      listenStreams = [ "/run/agent-board/api.sock" ];
      socketConfig = {
        Service = "agent-board.service";
        FileDescriptorName = "api";
        SocketMode = "0666";
        DirectoryMode = "0755";
      };
    };
    systemd.sockets.agent-board-html = socketDefaults // {
      listenStreams = [ "/run/agent-board/html.sock" ];
      socketConfig = {
        Service = "agent-board.service";
        FileDescriptorName = "html";
        SocketUser = "agent-board";
        SocketGroup = cfg.htmlGroup;
        SocketMode = "0660";
        DirectoryMode = "0755";
      };
    };
    systemd.sockets.agent-board-http = lib.mkIf (cfg.httpAddress != null) (socketDefaults // {
      listenStreams = [ cfg.httpAddress ];
      socketConfig = {
        Service = "agent-board.service";
        FileDescriptorName = "http";
        # wg0 may come up after the socket.
        FreeBind = true;
      };
    });

    systemd.services.agent-board = {
      description = "Agent board";
      requires = [ "agent-board-api.socket" "agent-board-html.socket" ]
        ++ lib.optional (cfg.httpAddress != null) "agent-board-http.socket";
      after = [ "agent-board-api.socket" "agent-board-html.socket" ]
        ++ lib.optional (cfg.httpAddress != null) "agent-board-http.socket";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${board}/bin/agent-board serve --db ${stateDir}/board.db --config ${configFile}";
        Sockets = [ "agent-board-api.socket" "agent-board-html.socket" ]
          ++ lib.optional (cfg.httpAddress != null) "agent-board-http.socket";
        LoadCredential = lib.mapAttrsToList (agent: file: "token-${agent}:${file}") cfg.tokens;
        User = "agent-board";
        Group = "agent-board";
        StateDirectory = [ "agent-board" "agent-board/backup" ];
        StateDirectoryMode = "0700";
        Restart = "always";
        RestartSec = 5;
        # The sockets come from systemd, so the service itself never needs the network.
        PrivateNetwork = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
        CapabilityBoundingSet = "";
        UMask = "0077";
        MemoryMax = "1G";
      };
    };

    # A consistent nightly copy (VACUUM INTO) for an offsite backup to pick up; zrepl
    # snapshots the live database as well.
    systemd.services.agent-board-backup = {
      description = "Agent board: nightly database copy";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${board}/bin/agent-board backup --db ${stateDir}/board.db --dir ${stateDir}/backup --keep 7";
        User = "agent-board";
        Group = "agent-board";
        StateDirectory = [ "agent-board" "agent-board/backup" ];
        StateDirectoryMode = "0700";
        PrivateNetwork = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        UMask = "0077";
      };
    };
    systemd.timers.agent-board-backup = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "*-*-* 04:30:00";
        Persistent = true;
      };
    };

    networking.firewall.interfaces.wg0.allowedTCPPorts = lib.mkIf (cfg.httpAddress != null)
      [ (lib.toInt (lib.last (lib.splitString ":" cfg.httpAddress))) ];
  };
}
