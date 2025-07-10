{ config, ... }: {
  services.syncplay.enable = true;
  networking.firewall.allowedTCPPorts = [ config.services.syncplay.port ];
}
