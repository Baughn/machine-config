# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
{ config
, pkgs
, ...
}: {
  imports = [
    ../modules
    ../modules/nvidia.nix
    ../modules/desktop.nix
    ./hardware-configuration.nix
  ];

  # Use the systemd-boot EFI boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  networking.networkmanager.enable = true;
  networking.hostName = "kaho";

  # Get some decent power management
  powerManagement.cpuFreqGovernor = "ondemand";
  hardware.nvidia.powerManagement.finegrained = true;
  powerManagement.powertop.enable = true;
}
