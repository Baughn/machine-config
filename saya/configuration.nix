# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

let
  userLib = pkgs.callPackage ../modules/users.nix {};
in

{
  imports = [
    ./hardware-configuration.nix
    ../modules/basics.nix
    ../modules/emergency-shell.nix
    ../modules/zfs.nix
    ../modules/desktop.nix
    ../modules/plex.nix
    ../modules/virtualisation.nix
    ../modules/nvidia.nix
    ../modules/rsyncd.nix
    ../modules/unifi.nix
  ];

  hardware.bluetooth.enable = true;
  hardware.pulseaudio.package = pkgs.pulseaudioFull.override {
    bluetoothSupport = true;
  };
  environment.etc."bluetooth/audio.conf".source = pkgs.writeText "audio.conf" ''
    [General]
    Enable = Source,Sink,Headset,Gateway,Control,Media
    Disable = Socket

    HFP=false

    [A2DP]
    SBCSources=1
    MPEG12Sources=0
  '';

#  ## Experimental fan control
#  boot.kernelPatches = [
#    { name = "it87.patch"; patch = ../third_party/it87/from-4.14.diff; }
#  ];
#  environment.systemPackages = [ pkgs.lm_sensors ];

  ## Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.kernelParams = [
    "boot.shell_on_fail"
    "nomodeset"
  ];
  systemd.enableEmergencyMode = true;
#  boot.kernelPackages = pkgs.linuxPackages_4_15;
#  boot.zfs.enableUnstable = true;

  # Development
  nix.extraOptions = ''
    gc-keep-outputs = true
    gc-keep-derivations = true
  '';

  ## Networking
  networking.hostName = "saya";
  networking.hostId = "7a4f1297";
  networking.bridges.br0 = {
    interfaces = [ "net" ];
  };
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="10:7b:44:92:13:2d", DEVPATH=="/devices/pci*", NAME="net"
  '';

  networking.interfaces.br0 = {
    useDHCP = true;
    # ipv4.addresses = [{ address = "192.168.1.42"; prefixLength = 24; }];
  };
  networking.firewall = {
    allowedTCPPorts = [ 
      6987  # rtorrent
    ];
    allowedUDPPorts = [
      6987  # rtorrent
      34197 # factorio
    ];
  };

  users = userLib.include [
    "will"
  ];
}
