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
    ./rolebot.nix
    ./sdbot.nix
    ./irctool.nix
    #../modules/home-assistant.nix
    #./syncplay.nix
    #    ./satisfactory.nix
    #./vintagestory.nix
    #    ./factorio.nix
    #    ./znc.nix
    #./unifi.nix
    #../modules/netboot-server.nix
    ../modules/nix-serve.nix
    ../modules/amdgpu.nix
    ../modules/nvidia.nix
    ../modules/zfs.nix
    ../modules/wireguard.nix
    ../modules/plex.nix
  ];

  me = {
    virtualisation.enable = true;
    monitoring.zfs = true;
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

  ## GPU
  hardware.nvidia.nvidiaPersistenced = true;

  ## AI?
  services.ollama = {
    enable = true;
    acceleration = "cuda";
  };
  environment.systemPackages = with pkgs; [ ollama ];

  ## Networking
  programs.mosh.enable = lib.mkForce false;
  networking.hostName = "tsugumi";
  networking.networkmanager.enable = true;

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

  # Silver Bullet
  services.silverbullet.enable = true;

  # Syncthing
  services.syncthing = {
    enable = true;
    openDefaultPorts = true;
    user = "svein";
    configDir = "/home/svein/.config/syncthing";
    dataDir = "/home/svein/Sync";
    settings = {
      devices.saya.id = "5AAHVO7-OIPPJXL-ATSWRLI-AQPXU5A-ED3IYSU-IQD56VF-74NHQE2-CLGZUA6";
      devices.sayanix.id = "AWYR3YS-GQORA3W-MSRLQGB-MC6X3K4-FIBGBKF-IRJPM3P-QKCNRFP-M3CYNAN";
      devices.kaho.id = "MD47JRV-UL5JHDJ-VHSSSEC-OPAQGRS-X5MEAH3-MBJUBCO-WG3XIZA-7ZX2KQU";
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
  };

  ## Hardware
  # UPS
  power.ups = {
    #enable = true;
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
    upsmon = {
      #enable = true;
      monitor.phoenix = {
        user = "admin";
        passwordFile = config.age.secrets."nut/upspw".path;
      };
      settings.SHUTDOWNCMD = config.age.secrets."nut/do_shutdown.sh".path;
    };
    users.admin = {
      actions = ["ALL"];
      instcmds = ["ALL"];
      passwordFile = config.age.secrets."nut/upspw".path;
    };
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
    enable = false;
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

  ## monitoring
  services.grafana = {
    enable = true;
    settings.server = {
      enable_gzip = true;
      domain = "grafana.brage.info";
      http_port = 1230;
    };
  };

  services.prometheus = {
    enable = true;
    exporters.blackbox = {
      enable = true;
      configFile = ../modules/monitoring/blackbox.yml;
    };
    scrapeConfigs = [
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
    depends = ["/home/svein/Media"];
    options = ["bind"];
  };
  fileSystems."/srv/svein/Movies" = {
    device = "/home/svein/Movies";
    depends = ["/home/svein/Media"];
    options = ["bind"];
  };
  fileSystems."/srv/svein/TV" = {
    device = "/home/svein/TV";
    depends = ["/home/svein/Media"];
    options = ["bind"];
  };
  fileSystems."/srv/svein/Sync" = {
    device = "/home/svein/Sync/Watched";
    depends = ["/home/svein/Sync/Watched"];
    options = ["bind"];
  };

  services.authelia = {
    instances.main = {
      enable = true;
      secrets.storageEncryptionKeyFile = config.age.secrets."authelia-storage-key".path;
      secrets.jwtSecretFile = config.age.secrets."authelia-jwt-key".path;
      settings = {
       theme = "light";
       default_2fa_method = "totp";
       log.level = "debug";
       #server.disable_healthcheck = true;
       authentication_backend = {
         file = {
           path = "/var/lib/authelia-main/users.yml";
         };
       };
       access_control.default_policy = "one_factor";
       session.domain = "brage.info";
       storage = {
         local = {
           path = "/var/lib/authelia-main/db.sqlite3";
         };
       };
       notifier.filesystem.filename = "/var/lib/authelia-main/notification.txt";
      };
    };
  };


  services.caddy = {
    enable = true;
    package = pkgs.caddy.withPlugins {
      plugins = [ "github.com/caddy-dns/cloudflare@v0.0.0-20240703190432-89f16b99c18e" ];
      hash = "sha256-JoujVXRXjKUam1Ej3/zKVvF0nX97dUizmISjy3M3Kr8=";
    };
    email = "sveina@gmail.com";
    environmentFile = config.age.secrets."caddy.env".path;
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

        tls {
          dns cloudflare {env.CLOUDFLARE_API_TOKEN}
          resolvers 1.1.1.1
        }

        log {
          output stderr
          format console
        }
      }

      # Authelia portal
      auth.brage.info {
        reverse_proxy localhost:9091
      }

      (password) {
        forward_auth localhost:9091 {
          uri /api/verify?rd=https://auth.brage.info/
          copy_headers Remote-User Remote-Groups Remote-Email Remote-Name
        }
      }

      (localonly) {
        @denied not remote_ip 89.101.222.210/29
        abort @denied
      }

      brage.info {
        root * /srv/svein/
        import headers
        file_server browse
      }

      madoka.brage.info {
        root * /srv/minecraft/
        import headers
        reverse_proxy /warmroast/* localhost:23000
        file_server browse
      }

      grafana.brage.info {
        import headers
        reverse_proxy http://localhost:1230
      }

      map.brage.info {
        import headers
        reverse_proxy http://127.0.0.1:8123
      }

      incognito.brage.info {
        import headers
        reverse_proxy http://127.0.0.1:8124
      }

      home.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:8123
      }

      comfyui.brage.info {
        import headers
        import password
        reverse_proxy http://saya.local:8188
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
        import password
        reverse_proxy https://znc.brage.info:4000 {
        }
      }

      obico.brage.info {
        import headers
        import password
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

      qbt.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:8080
      }

      todo.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:3000
      }

      sonarr.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:8989
      }

      radarr.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:7878
      }

      jellyfin.brage.info {
        import headers
        import password
        reverse_proxy http://localhost:8096
      }

      www.brage.info, tsugumi.brage.info {
        import headers
        redir https://brage.info/
      }

      store.brage.info {
        import localonly
        reverse_proxy http://localhost:5000
      }

      plex.brage.info {
        import headers
        reverse_proxy http://localhost:32400
      }
    '';
  };

  users.include = ["minecraft" "aquagon" "nixremote"];
  #users.include = ["pl" "aquagon" "will" "snowfire" "minecraft" "linuxgsm"];
}
