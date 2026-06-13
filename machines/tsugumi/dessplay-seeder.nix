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
          ${lib.concatMapStringsSep " " (r: "--media-root ${r}") cfg.mediaRoots}
      '';
      serviceConfig = {
        Type = "simple";
        User = "svein";
        Group = "users";
        Restart = "always";
        RestartSec = "10";
        LoadCredential = [ "seeder.password:${cfg.passwordFile}" ];
      };
      environment.RUST_LOG = "debug";
    };
  };
}
