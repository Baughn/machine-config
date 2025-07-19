{ pkgs, ... }:

{
  # Environment Variables
  home.sessionVariables = {
    EDITOR = "nvim";
  };

  # Add directories to PATH
  home.sessionPath = [
    "~/.cargo/bin"
  ];

  # Shell aliases
  home.shellAliases = {
    claude = "~/.claude/local/claude";
  };

  programs.zsh.enable = true;

  # Program Configurations
  programs.git = {
    enable = true;
    userName = "Svein Ove Aas";
    userEmail = "sveina@gmail.com";
    lfs.enable = true;
  };

  programs.jujutsu = {
    enable = true;
    ediff = true;
    settings = {
      user = {
        name = "Svein Ove Aas";
        email = "sveina@gmail.com";
      };
      ui = {
        default-command = "log";
        pager = "less -FRX";
      };
    };
  };

  programs.ssh = {
    enable = true;
    controlMaster = "auto";
    controlPath = "~/.ssh/control-%r@%h:%p";
    controlPersist = "10m";
    serverAliveInterval = 60;
    serverAliveCountMax = 3;
    extraConfig = ''
      ConnectTimeout 30
      TCPKeepAlive yes
      ConnectionAttempts 2
    '';
  };

  programs.tmux = {
    enable = true;
    escapeTime = 10;
    terminal = "screen-256color";
  };

  programs.rtorrent = {
    enable = true;
    settings = ''
      upload_rate = 204800
      download_rate = 2097152
      directory.default.set = ~/Downloads
      session.path.set = ~/.rtorrent
      protocol.encryption.set = allow_incoming,try_outgoing,enable_retry
    '';
  };

  # Do not modify unless you want to delete your home directory.
  home.stateVersion = "25.05";
}
