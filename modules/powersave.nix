{ config, pkgs, ... }:

{
  # Power saving settings.
  services.tlp = {
    enable = true;
    extraConfig = ''
      SOUND_POWER_SAVE_ON_AC=1
    '';
  };
  services.upower.enable = true;

  powerManagement = {
    enable = true;
    powertop.enable = true;
  };

  networking.networkmanager.wifi.powersave = true;
}
