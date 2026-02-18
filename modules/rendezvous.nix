{ config, pkgs, lib, inputs, ... }:

let
  cfg = config.services.rendezvous;
  cfgClaude = config.services."dessplay-claude";
  pkg = inputs.dessplay.packages.${pkgs.stdenv.hostPlatform.system}.rendezvous;
  claudePkg = inputs.dessplay.packages.${pkgs.stdenv.hostPlatform.system}.claude;
in

{
  options.services.rendezvous = {
    enable = lib.mkEnableOption "DessPlay rendezvous server";

    bind = lib.mkOption {
      type = lib.types.str;
      default = "[::]:4433";
      description = "Address to bind to";
    };

    passwordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the password file";
    };
  };

  options.services."dessplay-claude" = {
    enable = lib.mkEnableOption "DessPlay AI classification node";

    server = lib.mkOption {
      type = lib.types.str;
      description = "Rendezvous server address (IP:port or hostname:port)";
    };

    passwordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the rendezvous password file";
    };

    apiKeyFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the Anthropic API key file";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.enable {
      systemd.services.dessplay-rendezvous = {
        description = "DessPlay Rendezvous Server";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = "${pkg}/bin/dessplay-rendezvous --bind ${cfg.bind} --password-file \${CREDENTIALS_DIRECTORY}/rendezvous.password --data-dir \${STATE_DIRECTORY}";
          Restart = "always";
          RestartSec = "10";

          # Load the secret via systemd credentials so DynamicUser can read it.
          LoadCredential = "rendezvous.password:${cfg.passwordFile}";

          # Persistent state for cert.der + key.der (TOFU identity)
          StateDirectory = "dessplay-rendezvous";

          # Security settings
          DynamicUser = true;
          ProtectSystem = "strict";
          ProtectHome = true;
          NoNewPrivileges = true;
          RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        };
      };

      networking.firewall.allowedUDPPorts = [ 4433 ];
    })

    (lib.mkIf cfgClaude.enable {
      systemd.services.dessplay-claude = {
        description = "DessPlay AI Classification Node";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        script = ''
          export DESSPLAY_PASSWORD="$(< "$CREDENTIALS_DIRECTORY/rendezvous.password")"
          export ANTHROPIC_API_KEY="$(< "$CREDENTIALS_DIRECTORY/anthropic.key")"
          exec ${claudePkg}/bin/dessplay-claude --server ${cfgClaude.server}
        '';

        serviceConfig = {
          Restart = "always";
          RestartSec = "10";

          LoadCredential = [
            "rendezvous.password:${cfgClaude.passwordFile}"
            "anthropic.key:${cfgClaude.apiKeyFile}"
          ];

          StateDirectory = "dessplay-claude";

          # Security settings
          DynamicUser = true;
          ProtectSystem = "strict";
          ProtectHome = true;
          NoNewPrivileges = true;
          RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        };
      };
    })
  ];
}
