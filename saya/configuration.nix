# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
    ../modules
    ./hardware-configuration.nix
    ../modules/nvidia.nix
    ../modules/desktop.nix
#    ../modules/amdgpu.nix
#    ../modules/rsyncd.nix
#    ../modules/znapzend.nix
#    ../modules/monitoring.nix
  ];

  me = {
    virtualisation.enable = true;
  };

  ## Boot & hardware
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.systemd-boot.enable = true;
  boot.kernelParams = [
    "boot.shell_on_fail"
  ];
  systemd.enableEmergencyMode = true;

  # Development
  nix.extraOptions = ''
    gc-keep-outputs = true
    gc-keep-derivations = true
  '';

  ## Networking
  networking.hostName = "saya";
  networking.useDHCP = false;
  networking.networkmanager.enable = true;
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="f0:2f:74:8c:54:2d", NAME="internal"
  '';
  networking.interfaces.internal.wakeOnLan.enable = true;


  networking.firewall = {
    allowedTCPPorts = [ 
      6987   # rtorrent
    ];
    allowedUDPPorts = [
      6987   # rtorrent
      34197  # factorio
      10401  # Wireguard
    ];
  };

  users.include = [ "will" ];
}
