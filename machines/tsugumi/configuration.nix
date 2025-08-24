# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running 'nixos-help').
{ config
, pkgs
, lib
, ...
}: {
  imports = [
    ../../modules
    ./hardware-configuration.nix
    ./sdbot.nix
    ./caddy.nix
    ./sonarr.nix
    ./rolebot.nix
    ./irctool.nix
    ./aniwatch.nix
    ./minecraft.nix
    ./syncthing.nix
    ./silverbullet.nix
    ./nfs.nix
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
    # Enable THP
    postBootCommands = ''
      echo always > /sys/kernel/mm/transparent_hugepage/enabled
      echo defer > /sys/kernel/mm/transparent_hugepage/defrag
    '';
  };

  ## GPU
  hardware.nvidia.nvidiaPersistenced = true;

  ## Networking
  networking = {
    hostName = "tsugumi";
    enableLAN = true;
  };

  ## WireGuard
  me.wireguard.enable = true;

  ## Monitoring
  me.monitoring.enable = true;

  ## Victron Energy monitoring
  me.victron-monitor.enable = true;

  ## Redis
  me.redis.enable = true;

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

  # Additional users for tsugumi services
  users.include = [ "minecraft" "aquagon" ];

  # This value determines the NixOS release from which the default
  # settings for stateful data, like file locations and database versions
  # on your system were taken. It's perfectly fine and recommended to leave
  # this value at the release version of the first install of this system.
  # Before changing this value read the documentation for this option
  # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
  system.stateVersion = "23.05"; # Did you read the comment?
}
