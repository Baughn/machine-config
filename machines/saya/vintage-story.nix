{ config
, pkgs
, lib
, ...
}: {
  # Vintage Story multiplayer server ports
  # Default port is 42420 (configurable in serverconfig.json)
  networking.firewall.allowedTCPPorts = [
    42420 # Vintage Story
  ];
  networking.firewall.allowedUDPPorts = [
    42420 # Vintage Story
  ];
}
