{ config, pkgs, ... }:

{
  # Power saving settings.
  services.tlp = {
    #enable = true;
    extraConfig = ''
      # For testing, let's make sure we get the experience.
      TLP_DEFAULT_MODE=BAT
      TLP_PERSISTENT_DEFAULT=1

      SOUND_POWER_SAVE_ON_AC=1
      CPU_MAX_PERF_ON_BAT=100
      CPU_BOOST_ON_BAT=0
      WIFI_PWR_ON_AC=on
      DEVICES_TO_DISABLE_ON_STARTUP="bluetooth wwan"
    '';
  };
  services.upower.enable = true;

  powerManagement = {
    enable = true;
    powertop.enable = true;
    scsiLinkPolicy = "min_power";
  };
  hardware.bluetooth.powerOnBoot = false;

  networking.networkmanager.wifi.powersave = true;

  environment.systemPackages = [ pkgs.acpi ];
}
