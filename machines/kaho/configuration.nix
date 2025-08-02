{ config, pkgs, lib, inputs, ... }:

{
  # Darwin-specific configuration for kaho (macOS)

  # List packages installed in system profile
  environment.systemPackages = with pkgs; [
    neovim
    mosh
    mpv
    jujutsu
    git
    rtorrent
    rustup
    nodejs
    yt-dlp
  ];

  homebrew = {
    enable = true;
    caskArgs.appdir = "/Applications/Autonix";
    casks = [
      "syncplay"
      "ghostty"
    ];
  };

  # System identification
  networking.computerName = "kaho";
  system.primaryUser = "svein";

  # Define the user for nix-darwin
  users.users.svein = {
    name = "svein";
    home = "/Users/svein";
  };

  # Necessary because Determinate manages nix
  nix.enable = false;

  # Enable passwordless sudo
  security.sudo.extraConfig = ''
    svein ALL = (ALL) NOPASSWD: ALL
  '';

  # Set Git commit hash for darwin-version
  system.configurationRevision = inputs.self.rev or inputs.self.dirtyRev or null;

  # Used for backwards compatibility, please read the changelog before changing
  # $ darwin-rebuild changelog
  system.stateVersion = 6;

  # The platform the configuration will be used on
  nixpkgs.hostPlatform = "aarch64-darwin";
}
