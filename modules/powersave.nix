{ config, pkgs, ... }:

{
  # Power saving settings.
  services.tlp.enable = true;
  services.upower.enable = true;

  powerManagement = {
    enable = true;
    powertop.enable = true;
  };

  networking.networkmanager.wifi.powersave = true;
}
