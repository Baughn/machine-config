{ pkgs, ... }:

{
  environment.systemPackages = [
    pkgs.neovim
    pkgs.jujutsu
    pkgs.git
    pkgs.ripgrep
    pkgs.fd
    pkgs.psmisc
    pkgs.uv
    pkgs.comma
    pkgs.btop
    pkgs.python3
    pkgs.tcpdump
    pkgs.bubblewrap
  ];

  programs.htop.enable = true;
  programs.mtr.enable = true;
}
