{ config, pkgs, ... }:

{
  networking.firewall.allowedUDPPorts = [
    15777    # Satisfactory query
    15000    # Satisfactory beacon
    7777     # Satisfactory game
  ];

  systemd.services.satisfactory-autopdater = {
    description = "Check for updates of the Satisfactory dedicated server";
    after = [ "satisfactory.service" ];
    startAt = "*-*-* *:3/15";
    serviceConfig = {
      StateDirectory = "satisfactory-updater";
    };
    script = ''
      set -exu -o pipefail
      export HOME="$STATE_DIRECTORY"
      cd "$STATE_DIRECTORY"
      if [[ ! -e steam.data ]]; then
        ${pkgs.steamcmd}/bin/steamcmd +login anonymous +quit
      fi
      ${pkgs.steamcmd}/bin/steamcmd +app_info_print 1690800 +quit > steam.data
      cat steam.data \
        |  ${pkgs.pcre}/bin/pcregrep -M '"branches"\n(?<w>[[:blank:]]+){(.|\n)*?\n\g{w}}' \
        |  ${pkgs.pcre}/bin/pcregrep -M '"public"\n(?<w>[[:blank:]]+){(.|\n)*?\n\g{w}}' \
        > version.new
      rm steam.data
      if [[ ! -e version.old ]]; then
        cp version.new version.old
      fi
      if ${pkgs.diffutils}/bin/cmp -s version.old version.new; then
        echo 'No updates'
      else
        systemctl restart satisfactory.service
        cp version.new version.old
      fi
    '';
  };

  systemd.services.satisfactory = {
    description = "The Satisfactory dedicated server";
    wantedBy = ["multi-user.target"];
    after = ["network-online.target"];
    wants = ["network-online.target"];
    serviceConfig = {
      Restart = "always";
      StateDirectory = "satisfactory";
      DynamicUser = true;
    };
    script = ''
      set -exu -o pipefail
      export HOME="$STATE_DIRECTORY"
      cd "$HOME"
      ${pkgs.steamcmd}/bin/steamcmd +force_install_dir "$STATE_DIRECTORY" +login anonymous +app_update 1690800 validate +quit
      ${pkgs.steam-run}/bin/steam-run ./FactoryServer.sh
    '';
  };
}

