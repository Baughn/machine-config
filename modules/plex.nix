{ config, pkgs, ... }:

{
  services.plex.enable = true;
  networking.firewall.allowedTCPPorts = [
    32400 3005 8324 32469
  ];
  networking.firewall.allowedUDPPorts = [
    1900 5353 32410 32412 32413 32414
  ];
}
