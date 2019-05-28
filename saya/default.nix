# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
    ../modules
    ./hardware-configuration.nix
    ../modules/emergency-shell.nix
    ../modules/virtualisation.nix
    ../modules/nvidia.nix
    ../modules/rsyncd.nix
    ../modules/unifi.nix
    ../modules/znapzend.nix
    ../modules/monitoring.nix
  ];

  me = {
    desktop.enable = true;
  };

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
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="10:7b:44:92:13:2d", DEVPATH=="/devices/pci*", NAME="eth0"
  '';
  networking.bridges.br0 = {
    interfaces = [ "eth0" ];
  };
  networking.interfaces.br0 = {
    useDHCP = true;
  };

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

  # Wireguard link between my machines
  networking.wireguard = {
    interfaces.wg0 = {
      ips = [ "10.40.0.3/24" ];
      peers = [
        # Madoka
        {
          allowedIPs = [ "10.40.0.2/32" ];
          endpoint = "madoka.brage.info:10401";
          persistentKeepalive = 30;
          publicKey = "kTxN9HAb73WDJXRAq704cKs/WS5VJ23oSgaAWeVrvRQ=";
        }
        # Tsugumi
        {
          allowedIPs = [ "10.40.0.1/32" ];
          endpoint = "10.19.2.2:10401";
          persistentKeepalive = 30;
          publicKey = "H70HeHNGcA5HHhL2vMetsVj5CP7M3Pd/uI8yKDHN/hM=";
        }
      ];
      privateKeyFile = "/secrets/wg.key";
    };
  };

  users.include = [ "will" ];
}
