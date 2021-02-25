# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
    ../modules
    ./hardware-configuration.nix
    ./minecraft.nix
    ../modules/plex.nix
    # ../disnix/production/tsugumi-config.nix
    <nixpkgs/nixos/modules/profiles/headless.nix>
    # <nixpkgs/nixos/modules/profiles/hardened.nix>
    # ../modules/powersave.nix
  ];

  me = {
    propagateNix = true;
    virtualisation.enable = true;
  };

  # Syncthing
  services.syncthing = {
    enable = true;
    package = (import <nixos-unstable> {}).syncthing;
    openDefaultPorts = true;
    user = "svein";
    configDir = "/home/svein/.config/syncthing";
    dataDir = "/home/svein/Sync";
  };

  # Monitoring
  services.prometheus = {
    enable = true;
    exporters.node = {
      enable = true;
      enabledCollectors = [ "systemd" "zfs" ];
    };
    exporters.nginx = {
      enable = true;
    };
    exporters.wireguard = {
      enable = true;
    };
    exporters.blackbox = {
      enable = true;
      configFile = ../modules/monitoring/blackbox.yml;
    };
    scrapeConfigs = [{
      job_name = "minecraft";
      static_configs = [{
        labels.server = "erisia";
        targets = ["localhost:1223"];
      }];
    } {
      job_name = "blackbox";
      static_configs = [{
        targets = ["localhost:9115"];
      }];
      metrics_path = "/probe";
      params.module = ["icmp"];
      params.target = ["google.com"];
    }];
  };

  ## Backups ##
  services.zrepl = {
    enable = true;
    
    local.minecraft = {
      sourceFS = "minecraft";
      targetFS = "stash/zrepl";
      exclude = [
        "minecraft/erisia/dynmap"
        "minecraft/incognito/dynmap"
        "minecraft/testing/dynmap"
      ];
    };
  };

  # Work around #1915
  boot.kernel.sysctl."user.max_user_namespaces" = 100;

  # Use GRUB, with fallbacks. Once fallbacks are implemented.
  boot.loader.efi.canTouchEfiVariables = true;
  boot.loader.grub = {
    enable = true;
    zfsSupport = true;
    efiSupport = true;
    device = "nodev";
  };

  # Power management:
  #services.thermald.enable = true;
  powerManagement.enable = true;
  powerManagement.powertop.enable = true;

  ## Networking ##
  networking.hostName = "tsugumi";
  networking.hostId = "3b3fc025";
  networking.usePredictableInterfaceNames = false;
  networking.interfaces.eth0.useDHCP = true;

  networking.firewall.allowedTCPPorts = [
    80 443   # Web-server
    6986     # rtorrent
    139 445  # Samba
    25565    # Minecraft
    25566    # Minecraft (incognito)
    4000     # ZNC
    42420    # Vintage Story
  ];
  networking.firewall.allowedUDPPorts = [
    6987 6881  # rtorrent
    10401      # Wireguard
    27016      # Space Engineers
    34197      # Factorio
    137 138    # Samba
    42420    # Vintage Story
  ];
  networking.firewall.allowedUDPPortRanges = [ { from = 60000; to = 61000; } ];


  # Wireguard link between my machines
  networking.wireguard = {
    interfaces.wg0 = {
      ips = [ "10.40.0.1/24" ];
      listenPort = 10401;
      peers = [
        # Tromso
        {
          allowedIPs = [ "10.40.0.4/32" ];
          persistentKeepalive = 30;
          publicKey = "F8V/UkXUxnb+RCF3UePpJSO1opoSORDFv+dI2HqFQW8=";
        }
        # Kaho
        {
          allowedIPs = [ "10.40.0.5/32" ];
          publicKey = "7m0CNEXrI4/n8FUmERohFWv13Mr+DQJ+BHX0Pn+C6Bk=";
        }
      ];
      privateKeyFile = "/secrets/wg.key";
    };
  };

  ## Services ##
  # Samba
  services.samba = {
    enable = true;
    extraConfig = ''
      map to guest = bad user
      mangled names = no
      dos charset = UTF-8
      unix charset = UTF-8

      [homes]
      read only = no
    '';
  };
  # users.users.smbguest = {
  #     name = "smbguest";
  #     uid = config.ids.uids.smbguest;
  #     description = "SMB guest user";
  # };

  # ZNC
  #services.znc.enable = true;

  # Webserver (ACME, nginx)
  security.acme = {
    acceptTerms = true;
    certs = {
      "brage.info" = {
        email = "sveina@gmail.com";
        group = "nginx";
        postRun = "systemctl reload nginx.service";
        webroot = "/var/lib/acme/acme-challenge";
        extraDomains = {
          "madoka.brage.info" = null;
          "status.brage.info" = null;
          "grafana.brage.info" = null;
#          "tppi.brage.info" = null;
#          "alertmanager.brage.info" = null;
          "map.brage.info" = null;
          "incognito.brage.info" = null;
#          "tppi-map.brage.info" = null;
#          "cache.brage.info" = null;
          "znc.brage.info" = null;
#          "quest.brage.info" = null;
          "warmroast.brage.info" = null;
#          "hydra.brage.info" = null;
#          "pw.brage.info" = null;
#          "ll.ja13.org" = null;
#          "ctl.ll.ja13.org" = null;
        };
      };
    };
  };

  systemd.services.nginx = {
    serviceConfig.ProtectHome = false;
  };
  services.nginx = let 
    headers = ''
      add_header Strict-Transport-Security "max-age=31536000; includeSubdomains";
      add_header X-Clacks-Overhead "GNU Terry Pratchett";
      add_header X-Frame-Options "allow-from https://madoka.brage.info";
      add_header X-XSS-Protection "1; mode=block";
      add_header X-Content-Type-Options "nosniff";
      add_header Referrer-Policy "no-referrer-when-downgrade";
      add_header Content-Security-Policy-Report-Only "default-src 'self'; report-uri /__cspreport__;";

      limit_rate 3750000;
    '';
    base = x: {
      forceSSL = true;
      useACMEHost = "brage.info";
    } // x;
    proxy = port: base {
      locations."/".proxyPass = "http://127.0.0.1:" + toString(port) + "/";
    };
    proxyJared = port: base {
      locations."/".proxyPass = "http://10.211.72.90:" + toString(port) + "/";
    };
  in {
    enable = true;
    recommendedGzipSettings = true;
    recommendedOptimisation = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = true;
    sslDhparam = ./nginx/dhparams.pem;
    sslProtocols = "TLSv1.2 TLSv1.3";
    sslCiphers = "ALL:EECDH+aRSA+AESGCM:EDH+aRSA:EECDH+aRSA:+AES256:+AES128:+SHA1:!CAMELLIA:!SEED:!3DES:!DES:!RC4:!eNULL";
    appendHttpConfig = ''
      ${headers}
      etag on;
    '';
    virtualHosts = {
      "madoka.brage.info" = base {
        locations = {
          "/" = {
            root = "/home/minecraft/web";
            tryFiles = "\$uri \$uri/ =404";
            extraConfig = ''
              add_header Cache-Control "public";
              disable_symlinks off;
              autoindex on;
              ${headers}
              expires 1h;
            '';
          };
          "/warmroast/".proxyPass = "http://127.0.0.1:23000/";
          "/baughn/".extraConfig = "alias /home/svein/web/;";
          "/tppi/".extraConfig = "alias /home/tppi/web/;";
        };
      };
      "map.brage.info" = proxy 8123;
      "incognito.brage.info" = proxy 8124;
      "status.brage.info" = proxy 9090;
      "znc.brage.info" = base {
         locations."/" = {
           proxyPass = "https://127.0.0.1:4000";
           extraConfig = "proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;";
         };
      };
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
      "wiki.sufficientvelocity.com" = base {
        locations."/".proxyPass = "http://127.0.0.1:3300";
      };
    };
  };

  users.include = ["pl" "aquagon" "will" "snowfire" "minecraft" "linuxgsm"];
}
