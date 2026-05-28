{ pkgs, ... }:

{
  environment.systemPackages = with pkgs; [
    # Editor & VCS
    neovim
    git
    jujutsu
    difftastic
    mergiraf

    # Search & file tools
    ripgrep
    fd

    # System inspection & monitoring
    binutils  # Provides strings etc.
    btop
    pciutils
    psmisc
    sysstat

    # Networking
    socat
    tcpdump
    wget
    speedtest-cli
    inetutils
    weechat

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

    # Backups
    restic

    # Media
    mediainfo
    mpv
    rtorrent
    yt-dlp
  ];

  programs.htop.enable = true;
  programs.mtr.enable = true;
}
