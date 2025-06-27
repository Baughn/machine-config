{ config, lib, pkgs, ... }:

{
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
 };
}
