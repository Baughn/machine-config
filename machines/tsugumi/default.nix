{ config, ... }:

let
  sshKeys = import ../../lib/ssh-keys.nix;
  wireguardPeers = [
    {
      publicKey = "wjyoTvOuIvfM8NG8CDd2IiHouDp/c9G1Zx0WtFrGUgY=";
      allowedIPs = [ "10.171.0.6/32" ];
      persistentKeepalive = 25;
    }
    {
      publicKey = "6OMjCrzgoBe3iAnXGlhcce/za/poemekSpE95BuCmXc=";
      allowedIPs = [ "10.171.0.2/32" ];
      persistentKeepalive = 25;
    }
    {
      publicKey = "We6UVqoySg+bpp3tdVBpATZsdpuTH6/O1JeATcbfvVg=";
      allowedIPs = [ "10.171.0.4/32" ];
      persistentKeepalive = 25;
    }
    {
      publicKey = "S+U8WhWiLl9NOzvFb1QGZg6brrGpnAVp0dfrQ5PsrCk=";
      allowedIPs = [ "10.171.0.5/32" ];
      persistentKeepalive = 25;
    }
    {
      publicKey = "jNO3umuWeSEYCtXUXVvT8RAl6nDm5gIYIMwYhSVDFTw=";
      allowedIPs = [ "10.171.0.7/32" ];
      persistentKeepalive = 25;
    }
  ];
in
{
  imports = [
    ../../modules
    ./hardware-configuration.nix
    ./secrets.nix
    ./lan.nix
    ./caddy.nix
    ./sonarr.nix
    ./rolebot.nix
    ./irctool.nix
    ./aniwatch.nix
    ./minecraft.nix
    ./syncthing.nix
    ./silverbullet.nix
    ./nfs.nix
    ./monitoring.nix
    ./redis.nix
    ./rendezvous.nix
    ./victron-monitor.nix
    ./nbb-diag.nix
  ];

  ## Boot
  boot = {
    loader = {
      systemd-boot = {
        enable = true;
        memtest86.enable = true;
      };
      efi.canTouchEfiVariables = true;
    };
  };

  me.cachy-kernel.enable = true;

  ## GPU
  hardware.nvidia.nvidiaPersistenced = true;

  ## Networking
  networking.hostName = "tsugumi";
  me.security.enable = true;
  me.nixBuildBalancer = {
    enable = true;
    role = "agent";
    agentListen = "10.171.0.1:8765";
    agentCapacity = 16;
    openFirewall = true;
  };

  me.cloudflareDyndns = {
    enable = true;
    hostname = "brage.info";
    zone = "brage.info";
    tokenFile = config.age.secrets."cloudflare-dyndns-token".path;
  };

  ## WireGuard
  me.wireguard = {
    enable = true;
    address = [ "10.171.0.1/24" ];
    privateKeyFile = config.age.secrets."wireguard-tsugumi".path;
    listenPort = 51820;
    peers = wireguardPeers;
  };

  ## DessPlay Rendezvous
  services.rendezvous = {
    enable = true;
    passwordFile = config.age.secrets."rendezvous.key".path;
    anidbUserFile = config.age.secrets."anidb-user".path;
    anidbPasswordFile = config.age.secrets."anidb-password".path;
  };

  # Power management
  powerManagement.enable = true;
  powerManagement.powertop.enable = true;

  ## ZFS replication with zrepl
  services.zrepl = {
    enable = true;

    settings = {
      jobs = [
        {
          name = "backup-sink";
          type = "sink";
          serve = {
            type = "local";
            listener_name = "backup-sink";
          };
          root_fs = "stash/zrepl";
        }
        {
          name = "stash";
          type = "snap";
          snapshotting = {
            type = "periodic";
            prefix = "zrepl_";
            interval = "15m";
          };
          filesystems = {
            "stash/encrypted<" = true;
            "stash/encrypted/short-term<" = false;
            "stash/minecraft" = true;
          };
          pruning = {
            keep = [
              { type = "last_n"; count = 4; }
              { type = "grid"; grid = "1x1h(keep=all) | 24x1h | 14x1d | 4x30d"; regex = "^zrepl_"; }
            ];
          };
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
            "rpool/root/nix<" = false;
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
        }
      ];
    };
  };

  # Environmental
  time.timeZone = "Europe/Dublin";

  i18n.defaultLocale = "en_US.UTF-8";

  users.users.svein = {
    uid = 1000;
    extraGroups = [ "wheel" "systemd-journal" "dialout" "sonarr" ];
    createHome = false;
  };

  users.users.minecraft = {
    isNormalUser = true;
    uid = 1018;
    createHome = false;
    linger = true;
    openssh.authorizedKeys.keys = sshKeys.minecraft;
  };

  # This value determines the NixOS release from which the default
  # settings for stateful data, like file locations and database versions
  # on your system were taken. It's perfectly fine and recommended to leave
  # this value at the release version of the first install of this system.
  # Before changing this value read the documentation for this option
  # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
  system.stateVersion = "23.05"; # Did you read the comment?
}
