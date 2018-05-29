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
    ../modules/nvidia.nix
  ];

  ## Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.kernelParams = [
    "boot.shell_on_fail"
  ];
  systemd.enableEmergencyMode = true;

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
  };

  users = userLib.include [ "svein" ];

  ## Fonts
  fonts = {
    enableDefaultFonts = true;
  };

  services.xserver = {
    enable = true;
    desktopManager = {
      gnome3.enable = true;
    };
    displayManager.gdm.enable = true;
    xkbOptions = "ctrl:swapcaps";
    enableCtrlAltBackspace = true;
    exportConfiguration = true;

    inputClassSections = [''
      Identifier "Mouse Remap"
      MatchProduct "Mad Catz Mad Catz M.M.O.7 Mouse|M.M.O.7"
      MatchIsPointer "true"
      MatchDevicePath "/dev/input/event*"
      Option    "Buttons" "24"
      Option    "ButtonMapping" "1 2 3 4 5 0 0 8 9 10 11 12 0 0 0 16 17 7 6 0 0 0 0 0" 
      Option    "AutoReleaseButtons" "20 21 22 23 24" 
      Option    "ZAxisMapping" "4 5 6 7"
    ''];
  };

  hardware.pulseaudio = {
    enable = true;
  };

  hardware.opengl = {
    enable = true;
    s3tcSupport = true;
  };

  nixpkgs.config.allowUnfree = true;
}
