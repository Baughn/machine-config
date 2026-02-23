{ config, lib, pkgs, mylib, inputs, ... }:

let
  # Import nixpkgs-master for accessing bleeding-edge packages
  pkgs-master = import inputs.nixpkgs-master {
    inherit (pkgs) system;
    config.allowUnfree = true;
  };
in
{
  imports = [
    ./performance-desktop.nix
  ];

  # Custom packages overlay
  nixpkgs.overlays = [
    (final: prev: {
      faketorio = prev.luaPackages.callPackage ../tools/faketorio.nix { };
    })
  ];

  # Allow things that need real-time (like sound) to get real-time.
  security.rtkit.enable = true;

  boot.kernel.sysctl = {
    # Increase max_map_count for compatibility with modern games via Proton/Wine.
    "vm.max_map_count" = 2147483642;
    # Use the BBR congestion control algorithm for potentially better online gaming performance.
    "net.ipv4.tcp_congestion_control" = "bbr";
  };

  services = {
    # Display manager and desktop environment
    displayManager = {
      sddm.enable = true;
    };
    desktopManager.plasma6.enable = true;

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

  # Fonts
  fonts.enableDefaultPackages = true;
  fonts.packages = with pkgs; [
    ipafont
    kochi-substitute
  ];

  # Desktop applications
  environment.systemPackages = with pkgs;
    let
      desktopApps = builtins.fromJSON (builtins.readFile ./desktopApps.json);

      antigravity = mylib.versions.selectNewest [
        pkgs.antigravity
        pkgs-master.antigravity
      ];

      # Select the newest Vintage Story version from available sources
      # Listed in priority order: stable sources first, master last (lowest priority)
      vintagestory-latest = mylib.versions.selectNewest [
        pkgs.vintagestory # nixpkgs unstable version
        (pkgs.vintagestory.overrideAttrs (oldAttrs: rec {
          version = "1.21.5";
          src = pkgs.fetchurl {
            url = "https://cdn.vintagestory.at/gamefiles/stable/vs_client_linux-x64_${version}.tar.gz";
            hash = "sha256-dG1D2Buqht+bRyxx2ie34Z+U1bdKgi5R3w29BG/a5jg=";
          };
        }))
        pkgs-master.vintagestory # nixpkgs master version (lowest priority due to rebuild cost)
      ];
    in
    map (name: pkgs.${name}) desktopApps ++ [ vintagestory-latest antigravity ];
}
