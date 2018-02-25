# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

let
  userLib = pkgs.callPackage ../modules/users.nix {};
in

{
  imports = [ ./hardware-configuration.nix ];

  # Use the gummiboot efi boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  # Thermal control?
  #services.thermald.enable = true;

  ## Networking ##
  networking.hostName = "tsugumi";
  networking.hostId = "3b3fc025";
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="d0:50:99:c0:fd:fb", NAME="internal"
  '';

  networking.firewall.allowedTCPPorts = [
    80 443  # Web-server
    6986  # rtorrent
    139
  ];

  # VPN link to Uiharu.
  services.openvpn.servers.uiharu = {
    autoStart = true;
    config = builtins.readFile ../secrets/memespace-vpn/ovpn.conf;
    up = "route add -net 10.16.0.0/16 gw 10.16.128.1";
  };

  ## Services ##
  # # Samba
  # services.samba = {
  #   enable = false;
  #   shares = {
  #     public = {
  #       browseable = "yes";
  #       comment = "Music";
  #       "guest ok" = "yes";
  #       path = "/home/svein/Music/";
  #       "read only" = true;
  #    };
  #  };
  #  extraConfig = ''
  #    guest account = smbguest
  #    map to guest = bad user
  #  '';
  # };
  # users.users.smbguest = {
  #     name = "smbguest";
  #     uid = config.ids.uids.smbguest;
  #     description = "SMB guest user";
  # };

  # Nginx
  services.nginx = {
    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = true;
    sslDhparam = ./nginx/dhparams.pem;
    appendHttpConfig = ''
      add_header Strict-Transport-Security "max-age=31536000; includeSubdomains";
      add_header X-Clacks-Overhead "GNU Terry Pratchett";
    '';
    virtualHosts = let base = x: {
      forceSSL = true;
      enableACME = true;
    } // x; in {
      "brage.info" = base {
        default = true;
        serverAliases = [ "tsugumi.brage.info" "www.brage.info" ];
        locations."/".root = "/home/svein/web/";
        extraConfig = "autoindex on;";
      };
      "ar-innna.brage.info" = base {
        locations."/" = {
          root = "/home/aquagon/web/";
          extraConfig = "autoindex on;";
        };
      };
    };
  };

  users = userLib.include [
    "pl" "aquagon" "will"
  ];
}
