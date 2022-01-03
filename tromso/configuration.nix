# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
    ../modules
    ../modules/emergency-shell.nix
    ../modules/amdgpu.nix
    ./hardware-configuration.nix
  ];

  me.desktop.enable = true;

  # Use the gummiboot efi boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.timeout = 3;
  boot.loader.efi.canTouchEfiVariables = true;
  systemd.enableEmergencyMode = false;  # Start up no matter what, if at all possible.
  hardware.cpu.amd.updateMicrocode = true;

  users.include = [];

  services.plex.enable = true;
  services.plex.openFirewall = true;

  ## Networking ##
  networking.hostName = "tromso";
  networking.hostId = "5c118177";
  networking.interfaces.internal.useDHCP = true;
  services.udev.extraRules = ''
    ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="04:92:26:d8:4a:e3", NAME="internal"
  '';

  services.ddclient = {
    enable = true;
    verbose = true;
    username = "Vaughn";
    passwordFile = "/home/svein/nixos/secrets/dyndns";
    server = "members.dyndns.org";
    extraConfig = ''
      custom=yes, tromso.brage.info
    '';
  };

  networking.wireguard = {
    interfaces.wg0 = {
      ips = [ "10.40.0.4/24" ];
      listenPort = 10401;
      peers = [
        # Tsugumi
        {
          allowedIPs = [ "10.40.0.1/32" ];
          endpoint = "brage.info:10401";
          persistentKeepalive = 30;
          publicKey = "H70HeHNGcA5HHhL2vMetsVj5CP7M3Pd/uI8yKDHN/hM=";
        }
      ];
      privateKeyFile = "/secrets/wg.key";
    };
  };
}
