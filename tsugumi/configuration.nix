# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

{
  imports = [
    ../modules
    ./hardware-configuration.nix
    ./minecraft.nix
    #../modules/plex.nix
  ];

  me = {
    virtualisation.enable = true;
  };

  ## Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.kernelPackages = pkgs.linuxPackages_latest;

  ## Networking
  networking.hostName = "tsugumi";
  networking.domain = "brage.info";
  networking.hostId = "deafbeef";
  networking.useDHCP = false;
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="10:7b:44:92:17:20", NAME="external"
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="e8:4e:06:8b:85:8c", NAME="internal"
  '';
  # External
  networking.interfaces.external = {
    ipv4.addresses = [{
      address = "89.101.222.210";
      prefixLength = 29;
    }];
  };
  networking.defaultGateway = "89.101.222.209";
  networking.nameservers = [ "8.8.8.8" "8.8.4.4" ];
  # Firewall
  networking.firewall.allowedTCPPorts = [
    80 443   # Web-server
    25565    # Minecraft
    25566    # Minecraft (incognito)
    4000     # ZNC
  ];
  networking.firewall.allowedUDPPorts = [
    10401      # Wireguard
    34197      # Factorio
  ];
  networking.firewall.allowedUDPPortRanges = [ { from = 60000; to = 61000; } ]; # mosh
  networking.firewall.interfaces.internal = {
    allowedTCPPorts = [
      139 445  # Samba
      5357     # winbindd
      22000    # Syncthing
    ];
    allowedUDPPorts = [
      137 138  # Samba
      3702     # winbindd
      21027    # Syncthing
    ];
  };
  # Internal
  networking.interfaces.internal = {
    ipv4.addresses = [{
      address = "10.0.0.1";
      prefixLength = 24;
    }];
  };
  networking.nat = {
    enable = true;
    externalInterface = "external";
    internalInterfaces = [ "internal" ];
  };
  services.dhcpd4 = {
    enable = true;
    extraConfig = ''
      option domain-name "brage.info";
      option domain-name-servers 8.8.8.8, 8.8.4.4;
      option routers 10.0.0.1;
      subnet 10.0.0.0 netmask 255.255.255.0 {
        range 10.0.0.100 10.0.0.200;
      }
    '';
    interfaces = [ "internal" ];
  };

  # Samba
  services.samba = {
    enable = true;
    extraConfig = ''
      map to guest = bad user
      mangled names = no

      [homes]
      read only = no
    '';
  };
  services.samba-wsdd.enable = true;

  # Syncthing
  services.syncthing = {
    enable = true;
    user = "svein";
    configDir = "/home/svein/.config/syncthing";
    dataDir = "/home/svein/Sync";
    declarative = {
      devices.saya.id = "N3YFTMR-SU7K5WL-CLVOU6U-ND57RFT-L2ZTH4D-DIET3WH-2KIMLGJ-R7EUXAF";
      folders."/home/svein/Sync" = {
        id = "default";
	devices = [ "saya" ];
      };
      #folders."/home/svein/Music" = {
      # id = "Music";
      # devices = [ "kaho" ];
      #};
    };
  };

  ## Hardware
  # UPS
  power.ups = {
    #enable = true;
    ups.phoenix = {
      description = "PhoenixTec VFI 2000";
      driver = "usbhid-ups";
      port = "auto";
      directives = [
        "default.battery.charge.low = 80"
        "default.battery.runtime.low = 2500"
        "ignorelb"
      ];
    };
  };

  # Power mgmt
  #services.thermald.enable = true;
  powerManagement.enable = true;
  powerManagement.powertop.enable = true;

  ## Backups ##
  services.zrepl2 = {
    enable = true;
    
    local.minecraft = {
      sourceFS = "rpool/minecraft";
      targetFS = "stash/zrepl";
      exclude = [
        "rpool/minecraft/erisia/dynmap"
        "rpool/minecraft/incognito/dynmap"
        "rpool/minecraft/testing/dynmap"
      ];
    };
  };

  ## Monitoring
  services.prometheus = {
    enable = true;
    exporters.node = {
      enable = true;
      enabledCollectors = [ "systemd" "zfs" ];
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


  # Work around #1915
  #boot.kernel.sysctl."user.max_user_namespaces" = 100;

  # Webserver (Caddy)
  fileSystems."/srv/aquagon" = {
    device = "/home/aquagon/web";
    options = [ "bind" ];
  };
  fileSystems."/srv/minecraft" = {
    device = "/home/minecraft/web";
    options = [ "bind" ];
  };
  fileSystems."/srv/svein" = {
    device = "/home/svein/web";
    options = [ "bind" ];
  };
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
        root * /srv/minecraft/
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
        root * /srv/aquagon/
        import headers
        file_server browse
      }

      brage.info {
        root * /srv/svein/
        import headers
        file_server browse
      }

      www.brage.info, tsugumi.brage.info {
        import headers
        redir https://brage.info/
      }
    '';
  };

  users.include = ["minecraft" "aquagon"];
  #users.include = ["pl" "aquagon" "will" "snowfire" "minecraft" "linuxgsm"];
}
