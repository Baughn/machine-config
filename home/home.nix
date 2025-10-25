{ pkgs, lib, ... }:

{
  # Environment Variables
  home = {
    sessionVariables = {
      EDITOR = "nvim";
      # Claude
      BASH_DEFAULT_TIMEOUT_MS = 300000;
      BASH_MAX_TIMEOUT_MS = 1800000;
    };

    # Add directories to PATH
    sessionPath = [
      "/home/svein/.cargo/bin"
      "/home/svein/.npm-global/bin"
    ];

    # Shell aliases
    shellAliases = {
      claude = "~/.claude/local/claude";
      codex = "npx @openai/codex@latest";
      za = "zellij a";
    };
  };

  # Program Configurations
  programs = {
    zsh.enable = true;

    git = {
      enable = true;
      settings.user = {
        name = "Svein Ove Aas";
        email = "sveina@gmail.com";
      };
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
      enableDefaultConfig = false;
      extraConfig = ''
        ConnectTimeout 30
        TCPKeepAlive yes
        ConnectionAttempts 2
      '';
      matchBlocks = {
        # Global options
        "*" = {
          controlMaster = "auto";
          controlPath = "~/.ssh/control-%r@%h:%p";
          controlPersist = "10m";
          serverAliveInterval = 60;
          serverAliveCountMax = 3;
        };
        # Smart connection to brage.info - tries IPv6 first
        "brage.info-ipv6" = {
          match = ''host brage.info exec "${../scripts/check-ipv6.sh} direct.brage.info"'';
          hostname = "direct.brage.info";
          port = 22;
          extraOptions = {
            AddressFamily = "inet6";
            ConnectTimeout = "5";
          };
        };

        # Fallback for brage.info when IPv6 is not available
        "brage.info-ipv4" = {
          match = ''host brage.info exec "! ${../scripts/check-ipv6.sh} direct.brage.info"'';
          hostname = "direct.brage.info";
          port = 22;
          proxyJump = "v4.brage.info";
          extraOptions = {
            AddressFamily = "inet";
          };
        };

        # Direct connection aliases for testing
        "direct.brage.info tsugumi" = {
          hostname = "direct.brage.info";
          port = 22;
          extraOptions = {
            AddressFamily = "inet6";
            ConnectTimeout = "5";
          };
        };

        # Proxy connection aliases for testing
        "proxy.brage.info tsugumi-proxy" = {
          hostname = "direct.brage.info";
          port = 22;
          proxyJump = "v4.brage.info";
          extraOptions = {
            AddressFamily = "inet";
          };
        };

        # Smart connection to saya.brage.info - tries IPv6 first
        "saya.brage.info-ipv6" = {
          match = ''host saya.brage.info exec "${../scripts/check-ipv6.sh} saya.brage.info"'';
          hostname = "saya.brage.info";
          port = 22;
          extraOptions = {
            AddressFamily = "inet6";
            ConnectTimeout = "5";
          };
        };

        # Fallback for saya.brage.info when IPv6 is not available
        "saya.brage.info-ipv4" = {
          match = ''host saya.brage.info exec "! ${../scripts/check-ipv6.sh} saya.brage.info"'';
          hostname = "saya.brage.info";
          port = 22;
          proxyJump = "v4.brage.info";
          extraOptions = {
            AddressFamily = "inet";
          };
        };

        # Direct connection alias for testing
        "direct.saya.brage.info saya" = {
          hostname = "saya.brage.info";
          port = 22;
          extraOptions = {
            AddressFamily = "inet6";
            ConnectTimeout = "5";
          };
        };

        # Proxy connection alias for testing
        "proxy.saya.brage.info saya-proxy" = {
          hostname = "saya.brage.info";
          port = 22;
          proxyJump = "v4.brage.info";
          extraOptions = {
            AddressFamily = "inet";
          };
        };
      };
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

    # direnv for automatic environment loading
    direnv = {
      enable = true;
      enableZshIntegration = true;
      nix-direnv.enable = true;
    };
  }; # End of programs block

  # Symlink claude files back to ~/.claude
  home.file = {
    ".claude/CLAUDE.md".source = ../context/CLAUDE.md;
    ".claude/agents" = {
      source = ../context/agents;
      recursive = true;
    };
  };


  # Do not modify unless you want to delete your home directory.
  home.stateVersion = "25.05";
}
