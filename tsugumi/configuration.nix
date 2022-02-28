# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, lib, ... }:

{
  imports = [
    ../modules
    ./hardware-configuration.nix
#    ./minecraft.nix
#    ./satisfactory.nix
#    ../modules/plex.nix
    ../modules/monitoring.nix
#    ./znc.nix
    #./unifi.nix
  ];

  me = {
    virtualisation.enable = false;
  };

  ## Boot
  boot.loader.systemd-boot = {
    enable = true;
    memtest86.enable = true;
  };
  boot.loader.efi.canTouchEfiVariables = true;

  # Enable THP
  boot.postBootCommands = ''
    echo always > /sys/kernel/mm/transparent_hugepage/enabled
    echo defer > /sys/kernel/mm/transparent_hugepage/defrag
  '';

  ## Networking
  services.openssh.openFirewall = false;
  services.avahi.enable = false;
  programs.mosh.enable = lib.mkForce false;
  networking.hostName = "tsugumi";
  networking.useDHCP = false;
  services.udev.extraRules = ''
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="3c:7c:3f:24:99:f6", NAME="external"
      ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="e8:4e:06:8b:85:8c", NAME="internal"
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
    80 443   # Web-server
#    25565    # Minecraft
#    25566    # Minecraft (incognito)
#    27500    # Stationeers
#    27015    # Stationeers
#    7777     # Terraria
  ];
  networking.firewall.allowedUDPPorts = [
#    10401    # Wireguard
#    34197    # Factorio
#    24454    # Minecraft (voice chat)
#    27500    # Stationeers
  ];
  networking.firewall.allowedUDPPortRanges = [
#    { from = 60000; to = 61000; }  # mosh
#    { from = 27015; to = 27020; }  # Steam
  ];
  networking.firewall.interfaces = let cfg = { 
    allowedTCPPorts = [
#      139 445  # Samba
#      5357     # winbindd
      22
      22000    # Syncthing
    ];
    allowedUDPPorts = [
#      137 138  # Samba
#      3702     # winbindd
      21027    # Syncthing
    ];
  }; in {
    internal = cfg;
    #wifi = cfg;
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
    externalIP = "89.101.222.211";
    internalInterfaces = [ "internal" ];
  };
  networking.nat.forwardPorts =
    let forward = port: [{
      destination = "10.0.0.191";
      proto = "udp";
      sourcePort = port;
    } {
      destination = "10.0.0.191";
      proto = "tcp";
      sourcePort = port;
    }];
    in pkgs.lib.concatMap forward ([
#      5100  # Elite
#      5200  # Stationeers
#      5201  # Stationeers
    ]);
  services.dhcpd4 = {
    enable = true;
    authoritative = true;
    machines = [{
      hostName = "saya";
      ethernetAddress = "f0:2f:74:8c:54:2d";
      ipAddress = "10.0.0.2";
    }];
    extraConfig = ''
      option domain-name "brage.info";
      option domain-name-servers 8.8.8.8, 8.8.4.4;
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
    #enable = true;
    initialScript = pkgs.writeText "synapse-init.sql" ''
      CREATE ROLE "matrix-synapse" WITH LOGIN PASSWORD '${builtins.readFile ../secrets/matrix-sql-pw}';
      CREATE DATABASE "matrix-synapse" WITH OWNER "matrix-synapse"
        TEMPLATE template0
        LC_COLLATE = "C"
        LC_CTYPE = "C";
    '';
  };
  
  services.matrix-synapse = {
    #enable = true;
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
  #services.hercules-ci-agent.enable = true;
  services.hercules-ci-agent.settings.concurrentTasks = 4;

  # Samba
  services.samba = {
    #enable = true;
    extraConfig = ''
      map to guest = bad user
      mangled names = no

      [homes]
      read only = no
    '';
  };
  #services.samba-wsdd.enable = true;

  # Syncthing
  services.syncthing = {
    enable = true;
    user = "svein";
    configDir = "/home/svein/.config/syncthing";
    dataDir = "/home/svein/Sync";
    devices.saya.id = "WU5AOBG-6BTZRCL-EDGE3IX-W6YQQHS-UE55TXD-7P3CBXH-NHM3VBI-6VULBQL";
    devices.kaho.id = "WO5QPPE-S37P4KO-L4KWQ23-SEV6VHB-ABTJ3JX-BB7ST2A-VCOBUMM-DEZKVAN";
    devices.koyomi.id = "WCPI5FZ-WOPAUNY-CO6L7ZR-KXP3BYN-NNOHZZI-K4TCWXM-2SNSFHW-QSA7MQM";
    folders."/home/svein/Sync" = {
      id = "default";
      devices = [ "saya" "kaho" "koyomi" ];
    };
    folders."/home/svein/Music" = {
     id = "Music";
     devices = [ "kaho" "koyomi" ];
    };
    folders."/home/svein/Documents" = {
     id = "Documents";
     devices = [ "kaho" "koyomi" ];
    };
    folders."/home/svein/secure" = {
      id = "secure";
      devices = [ "saya" "kaho" "koyomi" ];
    };
  };

  ## Hardware
  # UPS
  power.ups = {
    enable = true;
    ups.phoenix = {
      description = "PhoenixTec VFI 2000";
      driver = "nutdrv_qx";
      port = "/dev/ttyUSB0";
      directives = [
        "default.battery.packs = 24"
        "default.battery.type = PbAc"
        "default.battery.voltage.low = 46"
        "default.battery.voltage.high = 55.4"
        "runtimecal = 180,100,600,50"
        "default.battery.charge.low = 75"
        "default.battery.runtime.low = 1000"
        "ignorelb"
      ];
    };
  };
  # TODO: Fit this into the module.
  systemd.services.upsd.preStart = ''
    mkdir -p /var/lib/nut -m 0700
  '';
  environment.etc."nut/upsd.users".source = config.age.secrets."nut/upsd.users".path;
  environment.etc."nut/upsmon.conf".source = config.age.secrets."nut/upsmon.conf".path;
  environment.etc."nut/upsd.conf".text = "";
  environment.etc."nut/do_shutdown.sh" = {
    mode = "0555";
    source = config.age.secrets."nut/do_shutdown.sh".path;
  };
  # UPS monitoring
  systemd.services.prometheus-nut-exporter = let
    prometheus-nut-exporter = pkgs.rustPlatform.buildRustPackage rec {
      pname = "prometheus-nut-exporter";
      version = "0aedb7911e3019f9f137e99aa87bc5fa1936084c";

      src = pkgs.fetchFromGitHub {
        owner = "HON95";
        repo = pname;
        rev = version;
        sha256 = "sha256-iVXCdDcePGugVZT3wSLhd4RvLCMAFfiTKmLJmmn9FWA";
      };

      cargoSha256 = "sha256-5tvn7ahHTGqvwcAzVczwmYBX6bSvJNQWcESKJrP0SEc=";
    };
  in {
    description = "UPS status exporter";
    wantedBy = [ "multi-user.target" ];
    after = [ "upsd.service" "upsdrv.service" ];
    wants = [ "upsd.service" "upsdrv.service" ];
    serviceConfig = {
      Restart = "always";
      DynamicUser = true;
    };
    script = ''
      ${prometheus-nut-exporter}/bin/prometheus-nut-exporter
    '';
  };

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
      job_name = "nut";
      static_configs = [{
        targets = ["localhost:9995"];
      }];
      metrics_path = "/nut";
      params.target = ["localhost:3493"];
    }
    {
      job_name = "blackbox";
      static_configs = [{
        targets = ["localhost:9115"];
      }];
      metrics_path = "/probe";
      params.module = ["icmp"];
      params.target = ["google.com"];
    }];
    rules = [''
      - name: UPS
        rules:
        - alert: UPSLoadHigh
          expr: nut_load * 100 > 75
          for: 1m
          annotations:
            summary: "UPS power draw is above 75%"
            description: "Power draw is {{ $value}}%"
        - alert: UPSBadOutputVoltage
          expr: nut_output_volt > 232 or nut_output_volt < 228
          annotations:
            summary: "UPS is outputting bad voltage!"
            description: "UPS reports {{ $value }}V"
        - alert: UPSBadFrequency
          expr: nut_input_frequency_hertz < 49.5 or nut_input_frequency_hertz > 50.5
          annotations:
            summary: "UPS is reporting bad input frequency!"
            description: "UPS reports {{ $value }}Hz"
        - alert: UPSBadVoltage
          expr: nut_input_volts > 250 or nut_input_volts < 210
          annotations:
            summary: "UPS is reporting bad input voltage!"
            description: "UPS reports {{ $value }}V"
        - alert: UPSMissing
          expr: absent(nut_status) > 0
          for: 1m
          annotations:
            summary: "UPS appears to be missing!"
            description: "No data found by prometheus-nut-exporter"
        - alert: UPSOnBattery
          expr: nut_status != 1
          for: 1m
          annotations:
            summary: "UPS is running off battery"
            description: "nut_status is {{ $value }}"
        - alert: UPSChargeLow
          expr: nut_battery_charge * 100 < 80
          annotations:
            summary: "UPS charge state is low; shutdown imminent"
            description: "Charge: {{ $value }}%"
    ''];
  };

  # Work around #1915
  #boot.kernel.sysctl."user.max_user_namespaces" = 100;

  # Webserver (Caddy)
  fileSystems."/srv/aquagon" = {
    device = "/home/aquagon/web";
    depends = ["/home/aquagon/web"];
    options = [ "bind" ];
  };
  fileSystems."/srv/minecraft" = {
    device = "/home/minecraft/web";
    depends = ["/home/minecraft/web"];
    options = [ "bind" ];
  };
  fileSystems."/srv/svein" = {
    device = "/home/svein/web";
    depends = ["/home/svein/web"];
    options = [ "bind" ];
  };
  fileSystems."/srv/svein/Anime" = {
    device = "/home/svein/Anime";
    depends = ["/home/svein/Anime"];
    options = [ "bind" ];
  };
  services.caddy = {
    enable = true;
    email = "sveina@gmail.com";
    extraConfig = ''
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
