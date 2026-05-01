{ config, lib, pkgs, ... }:

let
  defaultTarget = "direct.brage.info";

  mappings = [
    "tcp:25565@${defaultTarget}"
    "tcp:25566@${defaultTarget}"
    "udp:27015@saya.brage.info"
    "udp:27016@saya.brage.info"
  ];

  v4proxy = pkgs.rustPlatform.buildRustPackage {
    pname = "v4proxy";
    version = "0.1.0";
    src = ./v4proxy;
    cargoHash = "sha256-ZqrgZtcKR81BTE6Qdt3TbltuKURu6uwWB6LDIyd3VfA=";
    meta = {
      description = "IPv4 to IPv6 proxy (TCP and UDP)";
      platforms = lib.platforms.linux;
    };
  };
in
{
  systemd.services.v4proxy = {
    description = "IPv4 to IPv6 Proxy";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];

    serviceConfig = {
      ExecStart = "${v4proxy}/bin/v4proxy --mappings '${lib.concatStringsSep "," mappings}' --default-target ${defaultTarget}";
      Restart = "always";
      RestartSec = "10";

      DynamicUser = true;
      AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
      CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];

      ProtectSystem = "strict";
      ProtectHome = true;
      NoNewPrivileges = true;
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
    };
  };

  networking.firewall.allowedTCPPorts = [ 25565 25566 ];
  networking.firewall.allowedUDPPorts = [ 27015 27016 ];
}
