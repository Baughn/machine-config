{ config, pkgs, lib, inputs, ... }:

let
  cfg = config.services.rendezvous;
  pkg = inputs.dessplay.packages.${pkgs.stdenv.hostPlatform.system}.rendezvous;
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

  config = lib.mkIf cfg.enable {
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
  };
}
