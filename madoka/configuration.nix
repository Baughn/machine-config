# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, lib, ... }:

let sshKeys = import ../modules/sshKeys.nix;
in

{
  imports =
    [ # Include the results of the hardware scan.
      ./hardware-configuration.nix
      ./minecraft.nix
      #./mediawiki.nix
    ];

  ## Boot ##
  # Use the GRUB 2 boot loader.
  boot.loader.grub.enable = true;
  boot.loader.grub.version = 2;
  # Define on which hard drive you want to install Grub.
  boot.loader.grub.devices = [ "/dev/sda" "/dev/sdb" ];
  # Start up if at all possible.
  systemd.enableEmergencyMode = false;

  security.pam.loginLimits = [
    {
      domain = "minecraft";
      type = "-";
      item = "memlock";
      value = "16777216";
    }
   ];

  ## Networking ##
  networking.hostName = "madoka";
  networking.hostId = "f7fcf93e";
 # networking.defaultGateway = "138.201.133.1";
  # Doesn't work due to missing interface specification.
  #networking.defaultGateway6 = "fe80::1";
  networking.localCommands = ''
    ${pkgs.nettools}/bin/route -6 add default gw fe80::1 dev enp0s31f6 || true
  '';
  networking.nameservers = [ "8.8.8.8" "8.8.4.4" ];
  networking.interfaces.enp0s31f6 = {
    ip6 = [{
      address = "2a01:4f8:172:3065::2";
      prefixLength = 64;
    }];
  };
  networking.firewall = {
    allowPing = true;
    allowedTCPPorts = [ 
      80 443  # Web-server
      25565 25566 25567  # Minecraft
      4000  # ZNC
      12345  # JMC's ZNC
    ];
    allowedUDPPorts = [
      34197 # Factorio
    ];
  };
  networking.nat = {
    enable = true;  # For mediawiki.
    externalIP = "138.201.133.39";
    externalInterface = "enp0s31f6";
    internalInterfaces = [ "ve-eln-wiki" ];
  };

  # Next free ID: 1019.
  users.extraUsers.svein = {
    isNormalUser = true;
    uid = 1004;
    extraGroups = [ "wheel" "docker" ];
    openssh.authorizedKeys.keys = sshKeys.svein;
  };
  users.extraUsers.mei = {
    isNormalUser = true;
    uid = 1017;
    openssh.authorizedKeys.keys = [
      "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDMZr+a9w5b6VyqkdNbepBSkjpKQBdAZHJRCRS9SnqVubQIhYg6WsYYOtjp0GKM2SK5MqcrLUQYiT2LDUFOtC4zFxgCcZ6IoAcVqDCiZH8oeDkZlo/Qv0boKovOAGxqf0IdC+KMDhDDMBczuhOECUonFi6ArsXg9JbRJ1xDkTvIftFyypcdM9LDFAxIEEMqx6iIu0ANLvSNvI4xLjiav9tP8+Ea5TPBTm4H0EpsJNA3YsQPuC5TvbVH0kIl1NTdG1CjeZgiMSAAQoNy15Ik1EDoCp/eRO9JzRYNmU4/Z+af5Ns6SP1e/uSYkxoQNneSSWzPRagdNbTN+wVKJ91qaFVL mei@mei-hevs"
    ];
  };
  users.extraUsers.einsig = {
    isNormalUser = true;
    uid = 1014;
    openssh.authorizedKeys.keys = sshKeys.einsig;
  };
  users.extraUsers.prospector = {
    isNormalUser = true;
    uid = 1015;
    openssh.authorizedKeys.keys = sshKeys.prospector;
  };
  users.extraUsers.minecraft = {
    isNormalUser = true;
    uid = 1000;
    extraGroups = [ ];
    openssh.authorizedKeys.keys = builtins.concatLists (lib.attrValues sshKeys);
  };
  users.extraUsers.tppi = {
    isNormalUser = true;
    uid = 1013;
    openssh.authorizedKeys.keys = builtins.concatLists [
      sshKeys.svein sshKeys.kim sshKeys.luke sshKeys.prospector
    ];
  };
  users.extraUsers.bloxgate = {
    isNormalUser = true;
    uid = 1001;
    extraGroups = [ ];
    openssh.authorizedKeys.keys = sshKeys.bloxgate;
  };
  users.extraUsers.buizerd = {
    isNormalUser = true; 
    uid = 1010;
    openssh.authorizedKeys.keys = sshKeys.buizerd;
  };
  users.extraUsers.darqen27 = {
    isNormalUser = true; 
    uid = 1007;
    extraGroups = [ ];
    openssh.authorizedKeys.keys = sshKeys.darqen27;
  };
  users.extraUsers.david = {
    isNormalUser = true; 
    uid = 1005;
  };
  users.extraUsers.jmc = {
    isNormalUser = true; 
    uid = 1003;
    extraGroups = [ ];
    shell = "/run/current-system/sw/bin/bash";
    openssh.authorizedKeys.keys = sshKeys.jmc;
  };
  users.extraUsers.kim = {
    isNormalUser = true; 
    uid = 1002;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keys = sshKeys.kim;
  };
  users.extraUsers.luke = {
    isNormalUser = true; 
    uid = 1006;
    extraGroups = [ ];
    openssh.authorizedKeys.keys = sshKeys.luke;
  };
  users.extraUsers.simplynoire = {
    isNormalUser = true; 
    uid = 1009;
  };
  users.extraUsers.vindex = {
    isNormalUser = true; 
    uid = 1011;
    extraGroups = [ ];
    openssh.authorizedKeys.keys=sshKeys.vindex;
  };
  users.extraUsers.xgas = {
    isNormalUser = true;
    uid = 1012;
    openssh.authorizedKeys.keys = sshKeys.xgas;
  };


  ## Webserver ##
  services.nginx = {
    package = pkgs.nginxMainline.override {
#      modules = with pkgs.nginxModules; [ njs dav moreheaders ];
    };
    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = true;
    sslDhparam = ./nginx/dhparams.pem;
    statusPage = true;
    appendHttpConfig = ''
      add_header Strict-Transport-Security "max-age=31536000; includeSubdomains";
      add_header X-Clacks-Overhead "GNU Terry Pratchett";
      autoindex on;
      etag on;

      # Fallback config for Erisia
      upstream erisia {
        server 127.0.0.1:8123;
# server unix:/home/minecraft/erisia/staticmap.sock backup;
      }
      # server {
      #   listen unix:/home/minecraft/erisia/staticmap.sock;
      #   location / {
      #     root /home/minecraft/erisia/dynmap/web;
      #   }
      # }
      # Ditto, Incognito.
      # TODO: Factor this. Perhaps send a PR or two.
      upstream incognito {
        server 127.0.0.1:8124;
  # server unix:/home/minecraft/incognito/staticmap.sock backup;
      }
      # server {
      #   listen unix:/home/minecraft/incognito/staticmap.sock;
      #   location / {
      #     root /home/minecraft/incognito/dynmap/web;
      #   }
      # }
      upstream tppi {
        server 127.0.0.1:8126;
        # server unix:/home/tppi/server/staticmap.sock backup;
      }
      # server {
      #   listen unix:/home/tppi/server/staticmap.sock;
      #   location / {
      #     root /home/tppi/server/dynmap/web;
      #   }
      # }
      
    '';
    virtualHosts = let
      base = locations: {
        forceSSL = true;
        enableACME = true;
        inherit locations;
      };
      proxy = port: base {
        "/".proxyPass = "http://localhost:" + toString(port) + "/";
      };
      root = dir: base {
        "/".root = dir;
      };
      minecraft = {
        root = "/home/minecraft/web";
        tryFiles = "\$uri \$uri/ =404";
        extraConfig = ''
          add_header Cache-Control "public";
          expires 1h;
        '';
      };
    in {
      "madoka.brage.info" = base {
        "/" = minecraft;
        "/warmroast".proxyPass = "http://localhost:23000/";
        "/baughn".extraConfig = "alias /home/svein/web;";
        "/tppi".extraConfig = "alias /home/tppi/web;";
      } // { default = true; };
      "brage-cdn.stream" = base {
        "/" = minecraft;
      };
      "kubernetes.brage.info" = base {
        "/" = {
          proxyPass = "https://localhost:444/";
          extraConfig = "proxy_ssl_verify off;";
        };
      };
      "status.brage.info" = proxy 9090;
      "grafana.brage.info" = proxy 3000;
      "tppi.brage.info" = root "/home/tppi/web";
      "alertmanager.brage.info" = proxy 9093;
      "map.brage.info" = base { "/".proxyPass = "http://erisia"; };
      "incognito.brage.info" = base { "/".proxyPass = "http://incognito"; };
      "tppi-map.brage.info" = base { "/".proxyPass = "http://tppi"; };
      "cache.brage.info" = root "/home/svein/web/cache";
      "znc.brage.info" = base { 
         "/" = {
           proxyPass = "https://localhost:4000";
           extraConfig = "proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;";
         };
      };
      "quest.brage.info" = proxy 2222;
      "warmroast.brage.info" = proxy 23000;
      "wiki.brage.info" = base {
        "/" = {
          proxyPass = "http://192.168.10.2";
          extraConfig = "rewrite ^(/)$ http://wiki.brage.info/wiki/ permanent;";
        };
      };
      "hydra.brage.info" = proxy 3001;
    };
  };
}
