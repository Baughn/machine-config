{ config, pkgs, lib, ... }:

let
  ports = [ 25565 25566 ];
  target = "direct.brage.info";
  portsComma = lib.concatStringsSep "," (map builtins.toString ports);
in 

{
  # Build the v4proxy package from source
  nixpkgs.overlays = [
    (self: super: {
      minecraft-ipv6-proxy = self.rustPlatform.buildRustPackage {
        pname = "minecraft-ipv6-proxy";
        version = "0.1.0";
        src = ../v4/v4proxy;
        cargoHash = "sha256-Eflcq1NlSBWFBtCra69TuU8AncuT/dflri0i3jrMxXI=";
        
        meta = with lib; {
          description = "IPv4 to IPv6 proxy for Minecraft servers";
          platforms = platforms.linux;
        };
      };
    })
  ];

  # Configure the systemd service
  systemd.services.minecraft-ipv6-proxy = {
    description = "Minecraft IPv4 to IPv6 Proxy";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];
    
    serviceConfig = {
      ExecStart = "${pkgs.minecraft-ipv6-proxy}/bin/v4proxy --ports ${toString portsComma} --target ${target}";
      Restart = "always";
      RestartSec = "10";
      
      # Security settings
      DynamicUser = true;
      AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
      CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];
      
      # Hardening
      ProtectSystem = "strict";
      ProtectHome = true;
      NoNewPrivileges = true;
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
    };
  };

  networking.firewall.allowedTCPPorts = ports;
}
