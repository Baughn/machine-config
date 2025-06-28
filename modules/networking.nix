{ config, lib, pkgs, ... }:

{
  options.networking.enableMDNS = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Enable mDNS (multicast DNS) and LLMNR for local network discovery";
  };

  config = {
    # Rename Intel 82599 10G NIC to 'lan' on any system
    services.udev.extraRules = ''
      # Intel 82599 10 Gigabit Network Connection
      SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x1557", NAME="lan"
    '';

    # Common settings
    networking = {
      hostId = "deafbeef";
      useDHCP = false;
      interfaces.lan.useDHCP = lib.mkDefault true;
      interfaces.lan.tempAddress = "disabled";
      networkmanager.enable = false;
      firewall.allowPing = true;
      firewall.allowedUDPPorts = lib.mkIf config.networking.enableMDNS [
        5353 # mDNS
        5355 # LLMNR
      ];
    };

    # mDNS configuration
    services.resolved = lib.mkIf config.networking.enableMDNS {
      extraConfig = ''
        MulticastDNS = yes
        LLMNR = yes
      '';
    };
  };
}
