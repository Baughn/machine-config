{ config, lib, pkgs, dessplay, ... }:

let
  cfg = config.services.rendezvous;
  pkg = dessplay.packages.${pkgs.stdenv.hostPlatform.system}.rendezvous;
in
{
  options.services.rendezvous = {
    enable = lib.mkEnableOption "DessPlay Rendezvous Server";
    bind = lib.mkOption {
      type = lib.types.str;
      default = "[::]:4433";
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
          export ANIDB_USER="$(< "$CREDENTIALS_DIRECTORY/anidb.user")"
          export ANIDB_PASSWORD="$(< "$CREDENTIALS_DIRECTORY/anidb.password")"
        ''}
        exec ${pkg}/bin/dessplay-rendezvous --bind ${cfg.bind} --data-dir "$STATE_DIRECTORY"
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
    };

    networking.firewall.allowedUDPPorts = [ 4433 ];
  };
}
