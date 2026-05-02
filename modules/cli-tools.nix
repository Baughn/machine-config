{ pkgs, ... }:

{
  environment.systemPackages = with pkgs; [
    neovim
    jujutsu
    git
    ripgrep
    fd
    psmisc
    uv
    comma
    btop
    python3
    tcpdump
    bubblewrap
    socat
    bun
    codex
  ];

  programs.htop.enable = true;
  programs.mtr.enable = true;
}
