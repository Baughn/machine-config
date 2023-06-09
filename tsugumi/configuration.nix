# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
{
  config,
  pkgs,
  lib,
  ...
}: {
  imports = [
    ../modules
    ./hardware-configuration.nix
    ./sonarr.nix
    ./minecraft.nix
    ./syncplay.nix
    #    ./satisfactory.nix
    ./vintagestory.nix
    #    ./factorio.nix
    ../modules/plex.nix
    ../modules/monitoring.nix
    #    ./znc.nix
    #./unifi.nix
    #../modules/netboot-server.nix
    ../modules/nix-serve.nix
    ../modules/amdgpu.nix
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

  # Enable THP
  boot.postBootCommands = ''
    echo always > /sys/kernel/mm/transparent_hugepage/enabled
    echo defer > /sys/kernel/mm/transparent_hugepage/defrag
    # Disable boost
    #echo 0 > /sys/devices/system/cpu/cpufreq/boost
  '';

  ## Networking
  services.avahi.enable = false;
  programs.mosh.enable = lib.mkForce false;
  networking.hostName = "tsugumi";
  networking.useDHCP = false;
  services.udev.extraRules = ''
    # Attempt to fix the bloody realtek drivers.
    ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="3c:7c:3f:24:99:f6", NAME="external", RUN+="${pkgs.ethtool}/bin/ethtool --change external autoneg off"
    ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="e8:4e:06:8b:85:8c", NAME="internal", RUN+="${pkgs.ethtool}/bin/ethtool --change internal autoneg off"
    # Set an appropriate usb device name for the FLSUN Super Racer serial port.
    ACTION=="add", SUBSYSTEM=="tty", ATTRS{serial}=="208734504D34", SYMLINK+="ttyFLSUNRacer"
  '';
  # External
  networking.interfaces.external = {
    ipv4.addresses = [
      {
        address = "89.101.222.210";
        prefixLength = 29;
      }
      {
        address = "89.101.222.211";
        prefixLength = 29;
      }
    ];
  };
  networking.defaultGateway = "89.101.222.209";
  networking.nameservers = ["8.8.8.8" "8.8.4.4"];
  # Wireguard
  networking.wg-quick = {
    interfaces.wg0 = {
      address = ["10.0.2.1"];
      peers = [
        {
          allowedIPs = ["10.0.2.2/32"];
          endpoint = "tromso.brage.info:51820";
          publicKey = (import ../secrets/wireguard/pubkeys.nix).tromso;
          persistentKeepalive = 30;
          presharedKeyFile = config.age.secrets."wireguard/common.psk".path;
        }
      ];
      listenPort = 51820;
      privateKeyFile = config.age.secrets."wireguard/tsugumi.pk".path;
    };
  };
  # Firewall
  networking.firewall.allowedTCPPorts = [
    80
    443 # Web-server
  ];
  networking.firewall.allowedUDPPorts = [
    34197  # Factorio
  ];
  networking.firewall.allowedUDPPortRanges = [
  ];

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
    devices.saya.id = "4YR3ALE-55UONK6-ABXCSXM-OKBZNIT-HAJKCXQ-DV2LXKH-TFWVLZV-HKHB6Q3";
    devices.sayanix.id = "AWYR3YS-GQORA3W-MSRLQGB-MC6X3K4-FIBGBKF-IRJPM3P-QKCNRFP-M3CYNAN";
    devices.kaho.id = "WO5QPPE-S37P4KO-L4KWQ23-SEV6VHB-ABTJ3JX-BB7ST2A-VCOBUMM-DEZKVAN";
    devices.koyomi.id = "WCPI5FZ-WOPAUNY-CO6L7ZR-KXP3BYN-NNOHZZI-K4TCWXM-2SNSFHW-QSA7MQM";
    folders."/home/svein/Sync" = {
      id = "default";
      devices = ["saya" "kaho" "koyomi" "sayanix"];
    };
    folders."/home/svein/Music" = {
      id = "Music";
      devices = ["kaho" "koyomi"];
    };
    folders."/home/svein/Documents" = {
      id = "Documents";
      devices = ["kaho" "koyomi" "sayanix"];
    };
    folders."/home/svein/secure" = {
      id = "secure";
      devices = ["saya" "kaho" "koyomi" "sayanix"];
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
    wantedBy = ["multi-user.target"];
    after = ["upsd.service" "upsdrv.service"];
    wants = ["upsd.service" "upsdrv.service"];
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
  services.zrepl = {
    enable = true;

    settings = {
      jobs = [{
        name = "backup-sink";
        type = "sink";
        serve = {
          type = "local";
          listener_name = "backup-sink";
        };
        root_fs = "stash/zrepl";
      }
      {
        name = "rpool";
        type = "push";
        connect = {
          type = "local";
          listener_name = "backup-sink";
          client_identity = "rpool";
        };
        replication.protection.incremental = "guarantee_incremental";
        snapshotting = {
          type = "periodic";
          prefix = "zrepl_";
          interval = "15m";
        };
        filesystems = {
          "rpool/minecraft/erisia/dynmap" = false;
          "rpool/minecraft/incognito/dynmap" = false;
          "rpool/minecraft/testing/dynmap" = false;
          "rpool/root<" = false;
          "rpool<" = true;
        };
        pruning = {
          keep_sender = [
            { type = "last_n"; count = 4; }
            { type = "grid"; grid = "1x1h(keep=all) | 24x1h | 7x1d"; regex = "^zrepl_"; }
          ];
          keep_receiver = [
            { type = "grid"; grid = "1x1h(keep=all) | 24x1h | 14x1d | 4x30d"; regex = "^zrepl_"; }
          ];
        };
      }];
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
        static_configs = [
          {
            targets = ["localhost:9995"];
          }
        ];
        metrics_path = "/nut";
        params.target = ["localhost:3493"];
      }
      {
        job_name = "blackbox";
        static_configs = [
          {
            targets = ["localhost:9115"];
          }
        ];
        metrics_path = "/probe";
        params.module = ["icmp"];
        params.target = ["google.com"];
      }
    ];
    rules = [
      ''
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
      ''
    ];
  };

  # Work around #1915
  #boot.kernel.sysctl."user.max_user_namespaces" = 100;

  # Webserver (Caddy)
  fileSystems."/srv/aquagon" = {
    device = "/home/aquagon/web";
    depends = ["/home/aquagon/web"];
    options = ["bind"];
  };
  fileSystems."/srv/minecraft" = {
    device = "/home/minecraft/web";
    depends = ["/home/minecraft/web"];
    options = ["bind"];
  };
  fileSystems."/srv/svein" = {
    device = "/home/svein/web";
    depends = ["/home/svein/web"];
    options = ["bind"];
  };
  fileSystems."/srv/svein/Anime" = {
    device = "/home/svein/Anime";
    depends = ["/home/svein/Anime"];
    options = ["bind"];
  };
  fileSystems."/srv/svein/Movies" = {
    device = "/home/svein/Movies";
    depends = ["/home/svein/Movies"];
    options = ["bind"];
  };
  fileSystems."/srv/svein/TV" = {
    device = "/home/svein/TV";
    depends = ["/home/svein/TV"];
    options = ["bind"];
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

      (password) {
        #@denied not remote_ip 89.101.222.210/29
        #abort @denied

        basicauth {
          svein JDJhJDE0JGEvMmIyM3o2Ty94b1dNdXNlNmFtYmVvUFJ5UmVaOExEU2tOdTlsNi9KSEZYVHZlbXFMYTBp
        }
      }

      (localonly) {
        @denied not remote_ip 89.101.222.210/29
        abort @denied
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

      alertmanager.brage.info {
        import headers
        import password
        reverse_proxy http://127.0.0.1:9093
      }

      znc.brage.info {
        import headers
        reverse_proxy https://znc.brage.info:4000 {
        }
      }

      obico.brage.info {
        import headers
        reverse_proxy http://localhost:3334
      }

      klipper.brage.info {
        import headers
        reverse_proxy /* http://10.92.71.49
        reverse_proxy /server/* http://10.92.71.49:7125 {
          header_up -Authorization
        }
        reverse_proxy /access/* http://10.92.71.49:7125 {
          header_up -Authorization
        }
        reverse_proxy /websocket http://10.92.71.49:7125 {
          header_up -Authorization
        }
        handle_path /moonraker/* {
          reverse_proxy http://10.92.71.49:7125 {
            header_up -Authorization
          }
        }
        import password
      }

      racer.brage.info {
        reverse_proxy http://localhost:5000
        import password
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

      qbt.brage.info {
        import headers
        reverse_proxy http://localhost:8080
      }

      sonarr.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:8989
      }

      jellyfin.brage.info {
        import headers
        reverse_proxy http://localhost:8096
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

      store.brage.info {
        import localonly
        reverse_proxy http://localhost:5000
      }
    '';
  };

  users.include = ["minecraft" "aquagon"];
  #users.include = ["pl" "aquagon" "will" "snowfire" "minecraft" "linuxgsm"];

  system.stateVersion = "21.11";
}
