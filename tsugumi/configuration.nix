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
    ./wireless-ap.nix
    ../modules/monitoring.nix
  ];

  me = {
    virtualisation.enable = true;
  };

  ## Boot
  boot.loader.systemd-boot = {
    enable = true;
    memtest86.enable = true;
  };
  boot.loader.efi.canTouchEfiVariables = true;

  ## Networking
  networking.hostName = "tsugumi";
  networking.domain = "brage.info";
  networking.hostId = "deafbeef";
  networking.useDHCP = false;
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="3c:7c:3f:24:99:f6", NAME="external"
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="e8:4e:06:8b:85:8c", NAME="internal-eth"
  '';
  # External
  networking.interfaces.external = {
    ipv4.addresses = [{
      address = "89.101.222.210";
      prefixLength = 29;
    } {
      address = "89.101.222.211";
      prefixLength = 29;
    }];
  };
  networking.defaultGateway = "89.101.222.209";
  networking.nameservers = [ "8.8.8.8" "8.8.4.4" ];
  # Firewall
  networking.firewall.allowedTCPPorts = [
    53       # Pihole
    80 443   # Web-server
    25565    # Minecraft
    25566    # Minecraft (incognito)
    4000     # ZNC
    7777     # Terraria
  ];
  networking.firewall.allowedUDPPorts = [
    53         # Pihole
    10401      # Wireguard
    34197      # Factorio
    24454    # Minecraft (voice chat)
  ];
  networking.firewall.allowedUDPPortRanges = [ { from = 60000; to = 61000; } ]; # mosh
  networking.firewall.interfaces = let cfg = { 
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
  }; in {
    internal = cfg;
    #wifi = cfg;
  };
  # WiFi / openwrt
  me.openwrt.enable = true;
  # Internal
  networking.bridges.internal.interfaces = [ "internal-eth" ];  # Also has wifi
  networking.interfaces.internal = {
    ipv4.addresses = [{
      address = "10.0.0.1";
      prefixLength = 24;
    }];
  };
  networking.nat = {
    enable = true;
    externalInterface = "external";
    externalIP = "89.101.222.211";
    internalInterfaces = [ "internal" ];
  };
  networking.nat.forwardPorts =
    let forward = port: [{
      destination = "10.0.0.100";
      proto = "udp";
      sourcePort = port;
    } {
      destination = "10.0.0.100";
      proto = "tcp";
      sourcePort = port;
    }];
    in pkgs.lib.concatMap forward [
      5100  # Elite
      5200  # Stationeers
      5201  # Stationeers
    ];
  services.dhcpd4 = {
    enable = true;
    extraConfig = ''
      option domain-name "brage.info";
      option domain-name-servers 10.0.0.1;
      option routers 10.0.0.1;
      subnet 10.0.0.0 netmask 255.255.255.0 {
        range 10.0.0.100 10.0.0.200;
      }
      subnet 10.0.1.0 netmask 255.255.255.0 {
        range 10.0.1.100 10.0.1.200;
      }
    '';
    interfaces = [ "internal" ];
  };

  # Matrix/Synapse
  services.postgresql = {
    enable = true;
    initialScript = pkgs.writeText "synapse-init.sql" ''
      CREATE ROLE "matrix-synapse" WITH LOGIN PASSWORD '${builtins.readFile ../secrets/matrix-sql-pw}';
      CREATE DATABASE "matrix-synapse" WITH OWNER "matrix-synapse"
        TEMPLATE template0
        LC_COLLATE = "C"
        LC_CTYPE = "C";
    '';
  };
  
  services.matrix-synapse = {
    enable = true;
    enable_metrics = true;
    enable_registration = false;
    allow_guest_access = false;
    registration_shared_secret = builtins.readFile ../secrets/matrix-registration.key;

    dynamic_thumbnails = true;
    listeners = [{
      bind_address = "89.101.222.210";
      port = 8448;
      resources = [{
        compress = false;
        names = ["client" "webclient" "federation"];
      }];
      tls = false;
      type = "http";
      x_forwarded = false;
    }];
    public_baseurl = "https://matrix.brage.info/";
    server_name = "brage.info";

    logConfig = ''
       version: 1

       # In systemd's journal, loglevel is implicitly stored, so let's omit it
       # from the message text.
       formatters:
           journal_fmt:
               format: '%(name)s: [%(request)s] %(message)s'

       filters:
           context:
               (): synapse.util.logcontext.LoggingContextFilter
               request: ""

       handlers:
           journal:
               class: systemd.journal.JournalHandler
               formatter: journal_fmt
               filters: [context]
               SYSLOG_IDENTIFIER: synapse

       root:
           level: WARNING
           handlers: [journal]

       disable_existing_loggers: False
     '';
  };

  # Hercules CI
  services.hercules-ci-agent.enable = true;
  services.hercules-ci-agent.concurrentTasks = 4;

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
      devices.saya.id = "WITNYHH-S7BTOYT-5FFXM5W-BKJASXO-GVCIOAF-GT7OUNI-PZDR6VL-7QWD6QY";
      devices.kaho.id = "CKVWFUQ-BMH5EP2-XQLPB34-M4UWQ43-MW7UKV4-UHGWTUB-M422Z2A-VCMHEQ2";
      folders."/home/svein/Sync" = {
        id = "default";
        devices = [ "saya" "kaho" ];
      };
      folders."/home/svein/Music" = {
       id = "Music";
       devices = [ "kaho" ];
      };
      folders."/home/svein/Documents" = {
       id = "Documents";
       devices = [ "kaho" ];
      };
    };
  };

  ## Hardware
  # UPS
  power.ups = {
    enable = true;
    ups.phoenix = {
      description = "PhoenixTec VFI 2000";
      driver = "usbhid-ups";
      port = "auto";
      directives = [
        "default.battery.charge.low = 80"
        "default.battery.runtime.low = 1000"
        "ignorelb"
      ];
    };
  };
  # TODO: Fit this into the module.
  systemd.services.upsd.preStart = ''
    mkdir -p /var/lib/nut -m 0700
  '';
  environment.etc."nut/upsd.users".source = "/home/svein/nixos/secrets/upsd.users";
  environment.etc."nut/upsmon.conf".source = "/home/svein/nixos/secrets/upsmon.conf";

  # Power mgmt
  #services.thermald.enable = true;
  powerManagement.enable = true;
  powerManagement.powertop.enable = true;

  ## Backups ##
  services.zrepl2 = {
    enable = true;
    
    local.ssd = {
      sourceFS = "rpool";
      targetFS = "stash/zrepl";
      exclude = [
        "rpool/minecraft/erisia/dynmap"
        "rpool/minecraft/incognito/dynmap"
        "rpool/minecraft/testing/dynmap"
	"rpool/root<"
      ];
    };
  };

  ## Monitoring
  services.prometheus = {
    enable = true;
    exporters.wireguard = {
      enable = true;
    };
    exporters.blackbox = {
      enable = true;
      configFile = ../modules/monitoring/blackbox.yml;
    };
    scrapeConfigs = [
    #{
    #  job_name = "minecraft";
    #  static_configs = [{
    #    labels.server = "erisia";
    #    targets = ["localhost:1223"];
    #  }];
    #}
    {
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
  fileSystems."/srv/svein/Anime" = {
    device = "/home/svein/Anime";
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

      matrix.brage.info {
        import headers
        reverse_proxy http://89.101.222.210:8448
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
