{ config, lib, pkgs, dessplay, ... }:

let
  cfg = config.services.dessplay-seeder;
  pkg = dessplay.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  options.services.dessplay-seeder = {
    enable = lib.mkEnableOption "DessPlay seeder";
    server = lib.mkOption {
      type = lib.types.str;
      default = "localhost:9876";
      description = "Rendezvous server to connect to (host[:port])";
    };
    username = lib.mkOption {
      type = lib.types.str;
      default = "tsugumi";
      description = "Display name the seeder presents to the rendezvous";
    };
    passwordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the file containing the shared authentication password";
    };
    mediaRoots = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = "Directories the seeder searches for existing files to serve";
    };
    cacheDir = lib.mkOption {
      type = lib.types.str;
      default = "/home/svein/.cache/dessplay/files";
      description = ''
        Directory for auto-fetched downloads. Kept separate from the media roots;
        it is auto-added as an additional media root at startup.
      '';
    };
    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/home/svein/.local/share/dessplay";
      description = ''
        Directory of the seeder's database (hash cache and library index), passed as --db.
        The default is where dessplay put it without --db.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.dessplay-seeder = {
      description = "DessPlay seeder";
      after = [ "network.target" "dessplay-rendezvous.service" ];
      wants = [ "dessplay-rendezvous.service" ];
      wantedBy = [ "multi-user.target" ];
      script = ''
        export DESSPLAY_PASSWORD="$(< "$CREDENTIALS_DIRECTORY/seeder.password")"
        exec ${pkg}/bin/dessplay --seeder \
          --server ${cfg.server} \
          --username ${cfg.username} \
          --cache-dir ${cfg.cacheDir} \
          --db ${cfg.dataDir}/dessplay.db \
          ${lib.concatMapStringsSep " " (r: "--media-root ${r}") cfg.mediaRoots}
      '';
      serviceConfig = {
        Type = "simple";
        User = "svein";
        Group = "users";
        Restart = "always";
        RestartSec = "10";
        LoadCredential = [ "seeder.password:${cfg.passwordFile}" ];
        # It only handles SIGINT; SIGTERM kills it without saying goodbye to the server.
        KillSignal = "SIGINT";

        # Hardening (Garibaldi's finding, Baughn asked, msg 1559355644744564828). It runs as
        # svein to read the media, so hide the rest of the home: it reads the media roots,
        # writes only its cache and database, spawns nothing and only talks QUIC (UDP) to the
        # rendezvous. All transfers are relayed through the rendezvous; no peer-to-peer.
        ProtectHome = "tmpfs";
        BindReadOnlyPaths = map (r: "-${r}") cfg.mediaRoots;
        BindPaths = [ cfg.cacheDir cfg.dataDir ];
        ProtectSystem = "strict";
        PrivateTmp = true;
        PrivateDevices = true;
        PrivateIPC = true;
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        ProcSubset = "pid";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
        # AF_NETLINK: glibc's getaddrinfo sorts multi-address answers (localhost) with it.
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" "AF_NETLINK" ];
      } // lib.optionalAttrs (lib.hasPrefix "localhost:" cfg.server) {
        # The rendezvous is local, so nothing else needs to be reachable (DNS is resolved's stub).
        IPAddressDeny = "any";
        IPAddressAllow = "localhost";
      };
      environment.RUST_LOG = "debug";
    };
  };
}
