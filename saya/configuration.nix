# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [ ./hardware-configuration.nix ];

  ## Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.kernelParams = [
    "boot.shell_on_fail"
    "nomodeset"
  ];
  # For Threadripper
  boot.kernelPackages = pkgs.linuxPackages_4_15;

  ## Networking
  networking.hostName = "saya";
  networking.hostId = "7a4f1297";

  networking.interfaces.enp5s0 = {
    useDHCP = true;
    ip4 = [{ address = "192.168.1.42"; prefixLength = 24; }];
  };
  networking.firewall = {
    allowedTCPPorts = [ 
      6987  # rtorrent
    ];
    allowedUDPPorts = [
      6987 34197
    ];
  };

  services.unifi.enable = true;

  ## Desktop
  users.extraUsers.svein.uid = 1000;
}
