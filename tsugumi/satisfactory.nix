{ config, pkgs, ... }:

{
  networking.firewall.allowedUDPPorts = [
    15777    # Satisfactory query
    15000    # Satisfactory beacon
    7777     # Satisfactory game
  ];
  systemd.services.satisfactory = {
    wantedBy = ["multi-user.target"];
    after = ["network-online.target"];
    wants = ["network-online.target"];
    serviceConfig = {
      RuntimeMaxSec = 43200;
      Restart = "on-abnormal";
      StateDirectory = "satisfactory";
      DynamicUser = true;
    };
    script = ''
      set -exu -o pipefail
      export HOME="$STATE_DIRECTORY"
      cd "$HOME"
      ${pkgs.steamcmd}/bin/steamcmd +force_install_dir "$STATE_DIRECTORY" +login anonymous +app_update 1690800 validate +quit
      ${pkgs.steam-run}/bin/steam-run ./FactoryServer.sh -multihome=89.101.222.210
    '';
  };
}

