{ pkgs, ... }:

{
  imports = [
    ./zsh.nix
    ./desktop.nix
    ./nvidia.nix
  ];

  # Security?
  security.sudo.wheelNeedsPassword = false;

  ## Nix settings
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
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
    neovim.defaultEditor = true;
  };

  # Shell configuration
  users.defaultUserShell = pkgs.zsh;

  # Software that I use virtually everywhere
  environment.systemPackages = with pkgs;
    let
      defaultApps = builtins.fromJSON (builtins.readFile ./defaultApps.json);
    in
    map (name: pkgs.${name}) defaultApps;
}
