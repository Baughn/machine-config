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
    ../modules/desktop.nix
    ../modules/amdgpu.nix
    ../modules/unifi.nix
  ];

  # Use the gummiboot efi boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.timeout = 3;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.supportedFilesystems = [ "zfs" ];
  systemd.enableEmergencyMode = false;  # Start up no matter what, if at all possible.
  hardware.cpu.amd.updateMicrocode = true;

  users = userLib.include [
    "anne" "znapzend"
  ];

  # HACK: Workaround the C6 bug.
  systemd.services.fix-zen-c6 = {
    description = "Work around the AMD C6 bug";
    path = [ pkgs.python ];
    wantedBy = [ "multi-user.target" ];
    script = ''
      python ${./zenstates.py} --c6-disable
    '';
  };
    

  ## Plex ##
  # services.plex.enable = true;
  services.nginx = {
#    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = true;
    sslDhparam = ./nginx/dhparams.pem;
    virtualHosts."tromso.brage.info" = {
      forceSSL = true;
      enableACME = true;
      locations."/".proxyPass = "http://localhost:32400";
    };
  };

  ## Networking ##
  networking.hostName = "tromso";
  networking.hostId = "5c118177";

  networking.firewall = {
    trustedInterfaces = [ "internal" ];
    allowedTCPPorts = [ 4242 80 ];
    allowedUDPPortRanges = [{from = 60000; to = 61000;}];
  };

  nixpkgs.config.allowUnfree = true;

  # Open up for znapzend.
  security.sudo.extraConfig = ''
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs list*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs recv -uF stash/backup/*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs get*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs destroy stash/backup/*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/test *
  '';
}
