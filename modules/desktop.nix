{ config, lib, pkgs, ... }:

{
  imports = [
    ./performance.nix
  ];

  # AIDEV-NOTE: Desktop/GUI specific configuration

  # Allow things that need real-time (like sound) to get real-time.
  security.rtkit.enable = true;

  boot.kernel.sysctl = {
    # Increase max_map_count for compatibility with modern games via Proton/Wine.
    "vm.max_map_count" = 2147483642;
    # Use the BBR congestion control algorithm for potentially better online gaming performance.
    "net.ipv4.tcp_congestion_control" = "bbr";
  };

  services = {
    ananicy.enable = true;

    # Display manager and desktop environment
    displayManager = {
      gdm = {
        enable = true;
        wayland = true;
      };
      autoLogin = {
        enable = true;
        user = "svein";
      };
    };
    desktopManager.gnome.enable = true;

    # Audio
    pipewire = {
      enable = true;
      pulse.enable = true;
    };

    # Package management
    flatpak.enable = true;
  };

  # Gaming
  programs.steam = {
    enable = true;

    # This override is the CRITICAL fix for GameMode integration.
    # It injects the gamemode package and its libraries directly into
    # Steam's sandboxed FHS environment, fixing the "libgamemode.so not found" error.
    package = pkgs.steam.override {
      extraPkgs = pkgs: with pkgs; [
        gamemode
      ];
    };
  };

  # Desktop applications
  environment.systemPackages = with pkgs; [
    google-chrome
    mpv
    syncplay
    vscode-fhs
  ];
}
