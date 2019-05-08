# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports =
    [
      ./hardware-configuration.nix
      ../modules
      ../modules/desktop.nix
    ];

  # Use the systemd-boot EFI boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.zfs.enableUnstable = true;
  boot.zfs.requestEncryptionCredentials = true;

  networking.hostName = "kaho"; # Define your hostname.
  networking.hostId = "a6825f89";
  networking.networkmanager.enable = true;

  # Tune power.
  powerManagement = {
    enable = true;
    powertop.enable = true;
  };
  networking.networkmanager.wifi.powersave = true;
  zramSwap.enable = true;

  # Select internationalisation properties.
  i18n = {
     consoleFont = "${pkgs.terminus_font}/share/consolefonts/ter-u28n.psf.gz";
  };

  environment.systemPackages = with pkgs; [
  ];

  # Enable sound.
  sound.enable = true;
  hardware.pulseaudio.enable = true;

  # Enable the X11 windowing system.
  services.xserver.enable = true;
  services.xserver.layout = "us";
  services.xserver.xkbOptions = "eurosign:e";
  services.xserver.videoDrivers = [ "intel" "modesetting" ];
  hardware.opengl = {
    enable = true;
    driSupport32Bit = true;
    s3tcSupport = true;
  };

  # Enable touchpad support.
  #services.xserver.libinput.enable = true;
  services.xserver.synaptics.enable = true;

  # Enable the KDE Desktop Environment.
  services.xserver.displayManager.sddm = {
    enable = true;
    enableHidpi = true;
    autoLogin.enable = true;
    autoLogin.user = "svein";
  };
  services.xserver.desktopManager.plasma5.enable = true;

  # This value determines the NixOS release with which your system is to be
  # compatible, in order to avoid breaking some software such as database
  # servers. You should change this only after NixOS release notes say you
  # should.
  system.stateVersion = "19.03"; # Did you read the comment?

}
