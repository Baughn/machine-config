{ pkgs, ... }:

{
  environment.systemPackages = with pkgs; [
    binutils  # Provides strings etc.
    neovim
    jujutsu
    git
    ripgrep
    fd
    psmisc
    uv
    btop
    python3
    tcpdump
    bubblewrap
    socat
    bun
    codex
    colmena
    pciutils
    rustup
  ];

  programs.htop.enable = true;
  programs.mtr.enable = true;
}
