{ config, lib, pkgs, ... }:

let
  # Custom Oh-My-Zsh theme package (shared with home-manager config)
  sunakunCustomTheme = pkgs.callPackage ../pkgs/zsh-sunaku-theme.nix { };
in
{
  programs.zsh = {
    enable = true;

    # Enable autosuggestions from history
    autosuggestions.enable = true;

    # Oh My Zsh configuration
    ohMyZsh = {
      enable = true;
      theme = "sunaku-custom";
      customPkgs = [ sunakunCustomTheme ];
      plugins = [
        "sudo"
        "git"
        "jj"
        "ssh"
      ];
    };
  };
}
