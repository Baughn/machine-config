{ pkgs, ... }:

{
  imports = [
    ./zsh.nix
    ./networking.nix
    ./users.nix
    ./tmux.nix
    ./monitoring.nix
    ./victron-monitor.nix
    ./performance-default.nix
  ];

  # Would prefer zram, but it's broken
  boot.tmp.cleanOnBoot = true;

  # Security?
  security.sudo.wheelNeedsPassword = false;

  # The default is 'performance', which is unnecessary.
  powerManagement.cpuFreqGovernor = "schedutil";

  ## Nix settings
  nix.settings = {
    auto-optimise-store = true;
    experimental-features = [ "nix-command" "flakes" ];
    trusted-users = [ "svein" ];
  };
  ## Using nix-index instead, for flake support
  programs = {
    command-not-found.enable = false;

    ## Non-nix development
    nix-ld = {
      enable = true;
      libraries = with pkgs; [
      ];
    };

    # Editor configuration
    neovim = {
      enable = true;
      defaultEditor = true;
      viAlias = true;
      vimAlias = true;
    };
  };

  # Automatic garbage collection
  nix.gc = {
    automatic = true;
    dates = "weekly";
    options = "--delete-older-than 14d";
  };

  # Services
  services.openssh.enable = true;
  services.resolved = {
    enable = true;
    dnssec = "allow-downgrade";
  };

  # Shell configuration
  users.defaultUserShell = pkgs.zsh;

  # Software that I use virtually everywhere
  environment.systemPackages = with pkgs;
    let
      cliApps = builtins.fromJSON (builtins.readFile ./cliApps.json);
    in
    map (name: pkgs.${name}) cliApps;

  # Users are now handled by users.nix with the users.include option
  users.include = [ "svein" ];
}
