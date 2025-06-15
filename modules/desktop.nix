{ config, lib, pkgs, ... }:

{
  # AIDEV-NOTE: Desktop/GUI specific configuration

  # Allow things that need real-time (like sound) to get real-time.
  security.rtkit.enable = true;
  services.ananicy.enable = true;

  boot.kernel.sysctl = {
    # Increase max_map_count for compatibility with modern games via Proton/Wine.
    "vm.max_map_count" = 2147483642;
    # Use the BBR congestion control algorithm for potentially better online gaming performance.
    "net.ipv4.tcp_congestion_control" = "bbr";
  };

  # Display manager and desktop environment
  services.displayManager.sddm.enable = true;
  services.displayManager.sddm.wayland.enable = true;
  services.displayManager.sddm.wayland.compositor = "kwin";
  services.desktopManager.plasma6.enable = true;

  # Audio
  services.pipewire = {
    enable = true;
    pulse.enable = true;
  };

  # Package management
  services.flatpak.enable = true;

  # Gaming
  programs.steam.enable = true;

  # Desktop applications
  environment.systemPackages = with pkgs; [
    google-chrome
    mpv
    syncplay
  ];
}
