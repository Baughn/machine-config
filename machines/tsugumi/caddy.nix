{ config, lib, pkgs, ... }:

{
  services.caddy = {
    enable = true;
    package = pkgs.caddy.withPlugins {
      plugins = [ "github.com/caddy-dns/cloudflare@v0.2.1" ];
      hash = "sha256-2D7dnG50CwtCho+U+iHmSj2w14zllQXPjmTHr6lJZ/A=";
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
        handle_path /images/* {
          reverse_proxy localhost:24464
        }
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

  # Open firewall for HTTP/HTTPS
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  # Bind-mount filesystems to allow access.
  fileSystems = {
    "/srv/aquagon" = {
      device = "/home/aquagon/web";
      depends = [ "/home/aquagon/web" ];
      options = [ "bind" ];
    };
    "/srv/minecraft" = {
      device = "/home/minecraft/web";
      depends = [ "/home/minecraft/web" ];
      options = [ "bind" ];
    };
    "/srv/svein" = {
      device = "/home/svein/web";
      depends = [ "/home/svein/web" ];
      options = [ "bind" ];
    };
    "/srv/svein/Anime" = {
      device = "/home/svein/Anime";
      depends = [ "/home/svein/Media" ];
      options = [ "bind" ];
    };
    "/srv/svein/Movies" = {
      device = "/home/svein/Movies";
      depends = [ "/home/svein/Media" ];
      options = [ "bind" ];
    };
    "/srv/svein/TV" = {
      device = "/home/svein/TV";
      depends = [ "/home/svein/Media" ];
      options = [ "bind" ];
    };
    "/srv/svein/Sync" = {
      device = "/home/svein/Sync/Watched";
      depends = [ "/home/svein/Sync/Watched" ];
      options = [ "bind" ];
    };
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
}
