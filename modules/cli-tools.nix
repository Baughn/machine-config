{ pkgs, diskoInstall, ... }:

{
  environment.systemPackages = (with pkgs; [
    # Editor & VCS
    neovim
    git
    jujutsu

    # Search & file tools
    ripgrep
    fd

    # System inspection & monitoring
    binutils  # Provides strings etc.
    btop
    pciutils
    psmisc

    # Networking
    socat
    tcpdump

    # Sandboxing
    bubblewrap

    # Languages & runtimes
    bun
    python3
    rustup
    uv

    # Deployment & AI tooling
    codex
    colmena

    # Boot & disk
    efibootmgr

    # Media
    mpv
    rtorrent
  ]) ++ [ diskoInstall ];

  programs.htop.enable = true;
  programs.mtr.enable = true;
}
