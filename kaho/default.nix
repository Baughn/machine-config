# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
      ./hardware-configuration.nix
      ../modules
      ../modules/desktop.nix
      ../modules/powersave.nix
      ../nixos-hardware/dell/xps/13-9380
  ];

  me = {
    # Use the default channel for less compilation.
    propagateNix = false;
    desktop = {
      enable = true;
      #wayland = true;
    };
  };

  boot.supportedFilesystems = [ "zfs" "f2fs" ];

  # Use the systemd-boot EFI boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.systemd-boot.consoleMode = "0";
  boot.loader.efi.canTouchEfiVariables = true;
  boot.zfs.enableUnstable = true;
  boot.zfs.requestEncryptionCredentials = true;
  #boot.kernelPackages = pkgs.linuxPackages_latest;

  networking.hostName = "kaho"; # Define your hostname.
  networking.hostId = "a6825f89";
  networking.networkmanager.enable = true;
  services.udev.packages = [ pkgs.crda ];

  zramSwap.enable = true;

  # Select internationalisation properties.
  i18n = {
     consoleFont = "${pkgs.terminus_font}/share/consolefonts/ter-u28n.psf.gz";
  };

  environment.systemPackages = with pkgs; [
    acpi
  ];

  # Enable sound.
  sound.enable = true;
  hardware.pulseaudio.enable = true;
  hardware.pulseaudio.extraModules = [
    pkgs.pulseaudio-modules-bt
  ];
  hardware.bluetooth.enable = true;

  # X11 settings.
  services.xserver.layout = "us";
  services.xserver.xkbOptions = "eurosign:e";
  services.xserver.videoDrivers = [ "intel" "modesetting" ];
  hardware.opengl = {
    enable = true;
    driSupport32Bit = true;
    s3tcSupport = true;
    extraPackages = [ pkgs.vaapiIntel ];
  };

  # Enable touchpad support.
  #services.xserver.libinput.enable = true;
  services.xserver.synaptics.enable = true;
}
