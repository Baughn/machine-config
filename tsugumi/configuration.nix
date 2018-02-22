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
    873  # rsync
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

  # Rsync
  services.rsyncd = {
    enable = true;
    motd = ''Welcome to Factorial Productions

    By accessing this site on the first day of the fourth month of the year 2018
    Anno Domini, you agree to grant Us a non transferable option to claim, for
    now and for ever more, your immortal soul. Should We wish to exercise this
    option, you agree to surrender your immortal soul, and any claim you may
    have on it, within 5 (five) working days of receiving written notification
    from "Baughn", or one of its duly authorised minions.

    '';
    modules =
    let module = config: ({
      "read only" = "yes";
      "use chroot" = "true";
      "uid" = "nobody";
      "gid" = "nobody";
    } // config); in {
      factorio = module {
        comment = "Factorio";
        path = "/home/svein/rsync/factorio";
      };
      incoming = module {
        comment = "Drop box";
        path = "/home/svein/rsync/incoming";
        "read only" = "false";
        "write only" = "true";
      };
    };
  };

  users = userLib.include [
    "pl" "aquagon" "will"
  ];
}
