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

  # Power management
  powerManagement.enable = true;
  powerManagement.powertop.enable = true;

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
