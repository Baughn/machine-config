{ config, lib, pkgs, ... }:

{
  options.networking.enableLAN = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Enable LAN configuration with Intel 82599 NIC, DHCP, and mDNS support";
  };

  config = lib.mkIf config.networking.enableLAN {
    # Rename network interfaces
    services.udev.extraRules = ''
      # Intel 82599 10 Gigabit Network Connection (spare NIC, may be reused)
      SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x1557", NAME="lan"
      
      # Intel E810 dual-port NIC - port 0 (device 0x159b)
      SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x159b", ATTR{phys_port_name}=="p0", NAME="p0_unused"
      
      # Intel E810 dual-port NIC - port 1 (device 0x159b)
      SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x159b", ATTR{phys_port_name}=="p1", NAME="lan"
      
      # Realtek RTL8125 motherboard ethernet (unused)
      SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x10ec", ATTRS{device}=="0x8125", NAME="rj45_unused"
      
      # MediaTek MT7921 WiFi adapter (unused)
      SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x14c3", ATTRS{device}=="0x0616", NAME="wifi_unused"
    '';

    # LAN network settings
    networking = {
      hostId = "deafbeef";
      useDHCP = false;
      interfaces.lan = {
        useDHCP = true;
        tempAddress = "disabled";

        # IPv4 local network with jumbo frames
        ipv4.routes = [
          {
            address = "192.168.0.0";
            prefixLength = 24;
            options.mtu = "9000";
          }
        ];

        # IPv6 local networks with jumbo frames  
        ipv6.routes = [
          {
            address = "2a02:8086:d05:6780::";
            prefixLength = 64;
            options.mtu = "9000";
          }
          {
            address = "fe80::";
            prefixLength = 64;
            options.mtu = "9000";
          }
        ];
      };
      networkmanager.enable = false;
      firewall.allowPing = true;
      firewall.allowedUDPPorts = [
        5353 # mDNS
        5355 # LLMNR
        34197 # Factorio
      ];
    };

    # mDNS configuration for local network discovery
    services.resolved = {
      extraConfig = ''
        MulticastDNS = yes
        LLMNR = yes
      '';
    };
  };
}
