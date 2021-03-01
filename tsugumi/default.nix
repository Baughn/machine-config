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

  # Webserver (Caddy)
  systemd.services.caddy.serviceConfig.ProtectHome = false;
  services.caddy = {
    enable = true;
    email = "sveina@gmail.com";
    config = ''
      (headers) {
        header Strict-Transport-Security "max-age=31536000; includeSubdomains"
        header X-Clacks-Overhead "GNU Terry Pratchett"
        header X-Frame-Options "allow-from https://madoka.brage.info"
        header X-XSS-Protection "1; mode=block"
        header X-Content-Type-Options "nosniff"
        header Referrer-Policy "no-referrer-when-downgrade"
        encode zstd gzip
        handle_errors {
          header content-type "text/plain"
          respond "{http.error.status_code} {http.error.status_text}"
        }
      }

      madoka.brage.info {
        root * /home/minecraft/web/
        import headers
        reverse_proxy /warmroast/* localhost:23000
        file_server browse
      }

      map.brage.info {
        import headers
        reverse_proxy http://127.0.0.1:8123
      }

      incognito.brage.info {
        import headers
        reverse_proxy http://127.0.0.1:8124
      }

      status.brage.info {
        import headers
        reverse_proxy http://127.0.0.1:9090
      }

      znc.brage.info {
        import headers
        reverse_proxy https://znc.brage.info:4000 {
        }
      }

      ar-innna.brage.info {
        root * /home/aquagon/web/
        import headers
        file_server browse
      }

      brage.info {
        root * /home/svein/web/
        import headers
        file_server browse
      }

      www.brage.info, tsugumi.brage.info {
        import headers
        redir https://brage.info/
      }
    '';
  };

  users.include = ["pl" "aquagon" "will" "snowfire" "minecraft" "linuxgsm"];
}
