{ pkgs, ... }:

{
  # Environment Variables
  home = {
    sessionVariables = {
      EDITOR = "nvim";
    };

    # Add directories to PATH
    sessionPath = [
      "~/.cargo/bin"
    ];

    # Shell aliases
    shellAliases = {
      claude = "~/.claude/local/claude";
    };
  };

  # Program Configurations
  programs = {
    zsh.enable = true;

    git = {
      enable = true;
      userName = "Svein Ove Aas";
      userEmail = "sveina@gmail.com";
      lfs.enable = true;
    };

    jujutsu = {
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

    ssh = {
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

    tmux = {
      enable = true;
      escapeTime = 10;
      terminal = "screen-256color";
    };

    rtorrent = {
      enable = true;
      extraConfig = ''
        upload_rate = 204800
        download_rate = 2097152
        directory.default.set = ~/Downloads
        session.path.set = ~/.rtorrent
        protocol.encryption.set = allow_incoming,try_outgoing,enable_retry
      '';
    };
  }; # End of programs block

  # Do not modify unless you want to delete your home directory.
  home.stateVersion = "25.05";
}
