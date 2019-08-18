{ config, pkgs, lib, ... }:

{
  # Power saving settings.
  services.tlp = {
    enable = true;
    extraConfig = ''
      TLP_DEFAULT_MODE=BAT

      SOUND_POWER_SAVE_ON_AC=1
      WIFI_PWR_ON_AC=on
      DEVICES_TO_DISABLE_ON_STARTUP="bluetooth wwan"

      CPU_SCALING_GOVERNOR_ON_BAT=powersave
      CPU_SCALING_GOVERNOR_ON_AC=powersave
      CPU_MAX_PERF_ON_BAT=200
      CPU_SCALING_MAX_FREQ_ON_BAT=4400000
      CPU_BOOST_ON_BAT=1

      AHCI_RUNTIME_PM_ON_BAT=auto
    '';
  };
  services.upower.enable = true;
  boot.kernelParams = [
    "workqueue.power_efficient=y"
  ];

  powerManagement = {
    enable = true;
    powertop.enable = true;
    scsiLinkPolicy = lib.mkIf (! config.services.tlp.enable) "min_power";
  };
  hardware.bluetooth.powerOnBoot = false;

  networking.networkmanager.wifi.powersave = true;

  environment.systemPackages = [ pkgs.acpi ];
}
