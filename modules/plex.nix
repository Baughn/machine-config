{
  config,
  pkgs,
  ...
}: {
  services.plex.enable = true;
  services.plex.openFirewall = false;
  networking.firewall.interfaces.internal = {
    allowedTCPPorts = [32400 3005 8324 32469];
    allowedUDPPorts = [1900 5353 32410 32412 32413 32414];
  };
}
