# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [ ./hardware-configuration.nix ];

  # Temp
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # Use the gummiboot efi boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.timeout = 3;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.supportedFilesystems = [ "zfs" ];
  systemd.enableEmergencyMode = false;  # Start up no matter what, if at all possible.

  ## Plex ##
  # services.plex.enable = true;
  services.nginx = {
    enable = true;
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
  networking.enableIPv6 = true;

  services.udev.extraRules = ''
    ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="30:85:a9:9e:c2:29", NAME="uplink"
    ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="68:05:ca:0e:7c:a3", NAME="internal"
  '';

  services.ddclient = {
    enable = true;
    domain = "tromso.brage.info";
    username = "Vaughn";
  };

  networking.firewall = {
    trustedInterfaces = [ "internal" ];
    allowedTCPPorts = [ 4242 80 ];
    allowedUDPPortRanges = [{from = 60000; to = 61000;}];
  };

  services.unifi.enable = true;

  # DHCPd
  services.dhcpd4 = {
    enable = false;
    interfaces = [ "internal" ];
    extraConfig = ''
      option routers 10.4.0.1;
      option domain-name-servers 8.8.8.8, 8.8.4.4;
      option domain-name "brage.info";
      subnet 10.4.0.0 netmask 255.255.0.0 {
        range 10.4.1.2 10.4.1.200;
      }
   '';
  };

  # Open up for znapzend.
  users.extraUsers.znapzend = {
    isNormalUser = true;
    uid = 1001;
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAW37vjjfhK1hBwHO6Ja4TRuonXchlLVIYnA4Px9hTYD svein@madoka.brage.info"
    ];
  };
  security.sudo.extraConfig = ''
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs list*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs recv -uF stash/backup/*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs get*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/zfs destroy stash/backup/*
    znapzend ALL= NOPASSWD: /run/current-system/sw/bin/test *
  '';

  ## Users ##
  users.extraUsers.svein = {
    uid = 1000;
  };
  # users.extraUsers.kim = {
  #   isNormalUser = true;
  #   uid = 1002;
  #   openssh.authorizedKeys.keys = [
  #     "ssh-rsa AAAAB3NzaC1yc2EAAAABIwAAAIEA3j9muBMkqIAQ8BLBK5Ki4I1l2gg//Yt/YLmZd6nAaqYO4OeZ50k7x4F1OFRnyWScDqb4C5XggG8FaBQVe5RfP43sKDFx6F9En/zPB0JwbWT7iVXlZHFLLqqZ+vzrEmEYexQSwftpR1neKWb39fZjOcZTvd7Tk3sGNbnr/0LMYW0= kim@localhost"
  #   ];
  # };
}
