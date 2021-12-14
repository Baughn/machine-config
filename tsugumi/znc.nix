{ config, pkgs, ... }:

{
  networking.firewall.allowedTCPPorts = [
    4000
  ];

  systemd.services.znc = {
    description = "ZNC bouncer";
    wantedBy = ["multi-user.target"];
    after = ["network-online.target"];
    wants = ["network-online.target"];
    serviceConfig = {
      User = "svein";
      ExecStart = "${pkgs.znc}/bin/znc -f";
      Restart = "always";
    };
  };
}

