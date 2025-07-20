# Edit this configuration file to define what should be installed on
# your system. Help is available in the configuration.nix(5) man page, on
# https://search.nixos.org/options and in the NixOS manual (`nixos-help`).

{ config, lib, pkgs, ... }:

{
  imports =
    [
      ./hardware-configuration.nix
      ./sdbot.nix
      ../../modules
      ../../modules/desktop.nix
      ../../modules/nvidia.nix
      ../../modules/secure-boot.nix
      ../../quirks/g903.nix
      ../../quirks/amd-x3d.nix
    ];

  # Boot
  boot = {
    loader.efi.canTouchEfiVariables = true;
    blacklistedKernelModules = [ "amdgpu" ];
    kernelParams = [ "boot.shell_on_fail" ];
  };

  # Enable emergency mode for troubleshooting
  systemd.enableEmergencyMode = true;

  # Hardware quirks
  programs.gamemode = {
    enable = true;

    settings.general = {
      # Pin game threads to the V-Cache cores (Logical cores 0-7 and their SMT siblings 16-23).
      pin_cores = "0-7,16-23";

      # Park the frequency cores (Logical cores 8-15 and their SMT siblings 24-31).
      # This makes them unavailable to the game, preventing stutter from inter-CCD traffic.
      park_cores = "8-15,24-31";
    };
  };

  # Networking
  networking = {
    hostName = "saya";
    enableLAN = true;
    firewall.allowedUDPPorts = [
      # Factorio
      34197
    ];
  };

  # Backup
  services.restic.backups.home = {
    user = "svein";
    passwordFile = config.age.secrets."restic.pw".path;
    repository = "sftp:svein@tsugumi.local:short-term/backups/saya";
    backupPrepareCommand = "${pkgs.restic}/bin/restic -r sftp:svein@tsugumi.local:short-term/backups/saya unlock";
    paths = [ "/home/svein" ];
    exclude = [
      # Enhanced exclusions from backup.sh
      "/home/*/.cache/*"
      "!/home/*/.cache/huggingface"
      "/home/*/.local/share/baloo/*"
      "/home/*/.local/share/Steam/steamapps"
      "**/shadercache"
      "**/Cache"
      "**/cache"
      "**/_cacache"
      "**/.venv"
      "**/venv"
      "**/ComfyUI/output"
    ];
    extraBackupArgs = [
      "--exclude-caches"
      "--compression=max"
      "--read-concurrency=4"
    ];
    timerConfig = {
      OnCalendar = "*:0/30"; # Every 30 minutes
    };
    pruneOpts = [
      "--keep-hourly=36"
      "--keep-daily=7"
      "--keep-weekly=4"
      "--keep-monthly=3"
    ];
  };

  # Environmental
  time.timeZone = "Europe/Dublin";

  # Custom tools
  environment.systemPackages = with pkgs; [
    (callPackage ../../tools/ping-discord { })
  ];

  # This option defines the first version of NixOS you have installed on this particular machine,
  # and is used to maintain compatibility with application data (e.g. databases) created on older NixOS versions.
  #
  # Most users should NEVER change this value after the initial install, for any reason,
  # even if you've upgraded your system to a new NixOS release.
  #
  # This value does NOT affect the Nixpkgs version your packages and OS are pulled from,
  # so changing it will NOT upgrade your system - see https://nixos.org/manual/nixos/stable/#sec-upgrading for how
  # to actually do that.
  #
  # This value being lower than the current NixOS release does NOT mean your system is
  # out of date, out of support, or vulnerable.
  #
  # Do NOT change this value unless you have manually inspected all the changes it would make to your configuration,
  # and migrated your data accordingly.
  #
  # For more information, see `man configuration.nix` or https://nixos.org/manual/nixos/stable/options#opt-system.stateVersion .
  system.stateVersion = "25.05"; # Did you read the comment?
}

