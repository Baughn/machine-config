{ config, lib, pkgs, dessplay, ... }:

let
  cfg = config.services.dessplay-oracle;
  pkg = dessplay.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  options.services.dessplay-oracle = {
    enable = lib.mkEnableOption "DessPlay oracle (answers `oracle:` questions in chat)";
    server = lib.mkOption {
      type = lib.types.str;
      default = "localhost:9876";
      description = "Rendezvous server to connect to (host[:port])";
    };
    username = lib.mkOption {
      type = lib.types.str;
      default = "oracle";
      description = "Chat name the oracle presents to the rendezvous";
    };
    passwordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the file containing the shared authentication password";
    };
    anthropicKeyFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the file containing the Anthropic API key";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.dessplay-oracle = {
      description = "DessPlay oracle";
      after = [ "network-online.target" "dessplay-rendezvous.service" ];
      wants = [ "network-online.target" "dessplay-rendezvous.service" ];
      wantedBy = [ "multi-user.target" ];
      script = ''
        export DESSPLAY_PASSWORD="$(< "$CREDENTIALS_DIRECTORY/oracle.password")"
        export ANTHROPIC_API_KEY="$(< "$CREDENTIALS_DIRECTORY/anthropic.key")"
        exec ${pkg}/bin/dessplay --oracle \
          --server ${cfg.server} \
          --username ${cfg.username} \
          --db "$STATE_DIRECTORY/oracle.db" \
          --cache-dir "$STATE_DIRECTORY/cache"
      '';
      serviceConfig = {
        Type = "simple";
        Restart = "always";
        RestartSec = "10";
        LoadCredential = [
          "oracle.password:${cfg.passwordFile}"
          "anthropic.key:${cfg.anthropicKeyFile}"
        ];
        StateDirectory = "dessplay-oracle";
        DynamicUser = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        # AF_UNIX/AF_NETLINK for name resolution (api.anthropic.com).
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" "AF_NETLINK" ];
      };
      environment.RUST_LOG = "info";
    };
  };
}
