# Edit this configuration file to define what should be installed on
# your system. Help is available in the configuration.nix(5) man page, on
# https://search.nixos.org/options and in the NixOS manual (`nixos-help`).

{ config, lib, pkgs, ... }:

{
  imports =
    [ # Include the results of the hardware scan.
      ./hardware-configuration.nix
    ];

  # Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  boot.blacklistedKernelModules = [ "amdgpu" ];

  # Networking
  networking.hostName = "saya";
  networking.hostId = "deafbeef";
  networking.interfaces.enp12s0.tempAddress = "enabled";
  networking.networkmanager.enable = true;
  services.openssh.enable = true;
  networking.firewall.allowedUDPPorts = [
    34197  # Factorio
  ];
  services.avahi = {
    enable = true;
    openFirewall = true;
    publish.enable = true;
    publish.addresses = true;
    publish.workstation = true;
  };

  # Environmental
  time.timeZone = "Europe/Dublin";
  security.sudo.wheelNeedsPassword = false;

  # Nix configuration
  nixpkgs.config.allowUnfree = true;
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  ## Using nix-index instead, for flake support
  programs.command-not-found.enable = false;

  # Desktop
  hardware.nvidia = {
    modesetting.enable = true;
    open = true;
  };
  services.displayManager.sddm.enable = true;
  services.displayManager.sddm.wayland.enable = true;
  services.displayManager.sddm.wayland.compositor = "kwin";
  services.desktopManager.plasma6.enable = true;
  services.xserver.videoDrivers = [ "nvidia" ];
  services.pipewire = {
    enable = true;
    pulse.enable = true;
  };
  services.flatpak.enable = true;

  programs.steam.enable = true;

  # Users
  users.users.svein = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
  };

  # System packages
  environment.systemPackages = with pkgs; [
    neovim wget restic sshfs google-chrome jujutsu nodejs git
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

