{ config, lib, pkgs, dessplay, ... }:

let
  cfg = config.services.rendezvous;
  pkg = dessplay.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  options.services.rendezvous = {
    enable = lib.mkEnableOption "DessPlay Rendezvous Server";
    bind = lib.mkOption {
      type = lib.types.str;
      default = "[::]:9876";
      description = "Address and port to bind to";
    };
    passwordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the file containing the authentication password";
    };
    anidbUserFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to the file containing the AniDB username";
    };
    anidbPasswordFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to the file containing the AniDB password";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.dessplay-rendezvous = {
      description = "DessPlay Rendezvous Server";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];
      script = ''
        export DESSPLAY_PASSWORD="$(< "$CREDENTIALS_DIRECTORY/rendezvous.password")"
        ${lib.optionalString (cfg.anidbUserFile != null) ''
          export DESSPLAY_ANIDB_USER="$(< "$CREDENTIALS_DIRECTORY/anidb.user")"
          export DESSPLAY_ANIDB_PASSWORD="$(< "$CREDENTIALS_DIRECTORY/anidb.password")"
        ''}
        exec ${pkg}/bin/dessplay-rendezvous \
          --listen ${cfg.bind} \
          --db "$STATE_DIRECTORY/rendezvous.db" \
          --cert-dir "$STATE_DIRECTORY"
      '';
      serviceConfig = {
        Restart = "always";
        RestartSec = "10";
        LoadCredential = [ "rendezvous.password:${cfg.passwordFile}" ]
          ++ lib.optionals (cfg.anidbUserFile != null) [
            "anidb.user:${cfg.anidbUserFile}"
            "anidb.password:${cfg.anidbPasswordFile}"
          ];
        StateDirectory = "dessplay-rendezvous";
        DynamicUser = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
      };
      environment.RUST_LOG = "debug";
    };

    networking.firewall.allowedUDPPorts = [ 9876 9877 ];
  };
}
