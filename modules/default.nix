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
  programs.command-not-found.enable = false;

  ## Non-nix development
  programs.nix-ld.enable = true;
  programs.nix-ld.libraries = with pkgs; [
  ];

  # Shell configuration
  users.defaultUserShell = pkgs.zsh;

  # Editor configuration
  programs.neovim.defaultEditor = true;

  # Software that I use virtually everywhere
  environment.systemPackages = with pkgs;
    let
      defaultApps = builtins.fromJSON (builtins.readFile ./defaultApps.json);
    in
    map (name: pkgs.${name}) defaultApps;
}
