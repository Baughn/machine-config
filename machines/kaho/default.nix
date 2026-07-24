{ pkgs, flakeSelf, ... }:

{
  imports = [ ./home.nix ];

  environment.systemPackages = with pkgs; [
    fd
    git
    jujutsu
    mosh
    mpv
    neovim
    nodejs
    ripgrep
    rtorrent
    rustup
    wget
    yt-dlp
    btop
  ];

  # GUI apps and a few libraries that want to live outside the nix store.
  homebrew = {
    enable = true;
    caskArgs.appdir = "/Applications/Autonix";
    brews = [
      "pkg-config"
      "openssl"
      "libwebm"
    ];
    taps = [ "BarutSRB/tap" ];
    casks = [
      "codex"
      "ghostty"
      "omniwm"
      "visual-studio-code"
      "crossover"
    ];
  };

  networking.computerName = "kaho";
  system.primaryUser = "svein";

  users.users.svein = {
    name = "svein";
    home = "/Users/svein";
  };

  # Determinate manages the Nix installation; nix-darwin must not fight it.
  nix.enable = false;

  security.sudo.extraConfig = ''
    svein ALL = (ALL) NOPASSWD: ALL
  '';

  system.configurationRevision = flakeSelf.rev or flakeSelf.dirtyRev or null;

  # Read the darwin-rebuild changelog before changing.
  system.stateVersion = 6;

  nixpkgs.hostPlatform = "aarch64-darwin";
}
