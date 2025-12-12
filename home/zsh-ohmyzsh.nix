# Oh My Zsh configuration for home-manager (used on Darwin)
# NixOS uses system-wide config in modules/zsh.nix instead
{ pkgs, ... }:

let
  # Custom Oh-My-Zsh theme package (shared with NixOS system-wide config)
  sunakunCustomTheme = pkgs.callPackage ../pkgs/zsh-sunaku-theme.nix { };
in
{
  programs.zsh = {
    autosuggestion.enable = true;

    oh-my-zsh = {
      enable = true;
      theme = "sunaku-custom";
      custom = "${sunakunCustomTheme}/share/zsh";
      plugins = [
        "sudo"
        "git"
        "jj"
        "ssh"
      ];
    };
  };
}
