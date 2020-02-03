# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
    ../modules
    ./hardware-configuration.nix
    ../modules/amdgpu.nix
  ];

  networking.networkmanager.enable = true;

  # Use the gummiboot efi boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.timeout = 3;
  boot.loader.efi.canTouchEfiVariables = true;
  systemd.enableEmergencyMode = false;  # Start up no matter what, if at all possible.
  hardware.cpu.amd.updateMicrocode = true;

  users.include = [ "anne" "znapzend" ];

  ## Plex ##
  services.plex.enable = true;
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
  networking.usePredictableInterfaceNames = false;
  networking.interfaces.eth0.useDHCP = true;

  networking.firewall = {
    allowedTCPPorts = [ 4242 80 8443 27036 27037 ];
    allowedUDPPortRanges = [{from = 60000; to = 61000;}];
    allowedUDPPorts = [ 10401 27031 27036 ];
  };

  services.ddclient = {
    enable = true;
    verbose = true;
    username = "Vaughn";
    password = (import ../secrets).dyndns;
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
