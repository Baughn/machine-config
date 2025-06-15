# Edit this configuration file to define what should be installed on
# your system. Help is available in the configuration.nix(5) man page, on
# https://search.nixos.org/options and in the NixOS manual (`nixos-help`).

{ config, lib, pkgs, ... }:

{
  imports =
    [
      ./hardware-configuration.nix
      ./sdbot.nix
      ../modules
    ];

  # Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.blacklistedKernelModules = [ "amdgpu" ];

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
  networking.hostName = "saya";
  networking.hostId = "deafbeef";
  networking.interfaces.enp12s0.tempAddress = "enabled";
  systemd.network.enable = true;
  networking.useDHCP = false;
  networking.useNetworkd = true;
  systemd.network.networks."10-enp12s0" = {
    matchConfig.Name = "enp12s0";
    networkConfig = {
      DHCP = "ipv4";
      IPv6AcceptRA = true;
      MulticastDNS = true;
      LinkLocalAddressing = false;
    };
  };
  services.openssh.enable = true;
  networking.firewall.allowedUDPPorts = [
    # mDNS
    5353
    5355
    # Factorio
    34197
  ];
  services.resolved = {
    enable = true;
    dnssec = "allow-downgrade";
    extraConfig = ''
      MulticastDNS = yes
      LLMNR = yes
    '';
  };

  # Environmental
  time.timeZone = "Europe/Dublin";

  # Users
  users.users.svein = {
    isNormalUser = true;
    extraGroups = [ "wheel" "gamemode" ];
  };

  # System packages
  environment.systemPackages = with pkgs; [
    neovim
    wget
    restic
    sshfs
    jujutsu
    nodejs
    git
    rustup
    ripgrep
    fd
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

