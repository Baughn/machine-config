{ pkgs, lib, config, isDarwin, isStandalone, colmenaPackage ? null, ... }:

let
  # Build the magic-reboot sender
  magic-reboot-send = pkgs.callPackage ../tools/magic-reboot/sender/default.nix { };

  # Wrapper that auto-injects the decrypted key path
  magic-reboot = pkgs.writeShellScriptBin "magic-reboot" ''
    exec ${magic-reboot-send}/bin/magic-reboot-send --key ${config.age.secrets.magic-reboot-key.path} "$@"
  '';

  # Custom Rust tools (standalone-only, NixOS machines get these via modules/default.nix)
  ping-discord = pkgs.callPackage ../tools/ping-discord { };
  network-monitor = pkgs.callPackage ../tools/network-monitor { };
in

{
  # Import standalone-only modules:
  # - zsh-ohmyzsh: NixOS uses system-wide config in modules/zsh.nix
  # - neovim: NixOS uses modules/neovim.nix
  # - tmux: NixOS uses modules/tmux.nix
  imports = lib.optionals isStandalone [ ./zsh-ohmyzsh.nix ./neovim.nix ./tmux.nix ];

  # Environment Variables
  home = {
    sessionVariables = {
      EDITOR = "nvim";
      # Claude
      BASH_DEFAULT_TIMEOUT_MS = 300000;
      BASH_MAX_TIMEOUT_MS = 1800000;
    } // lib.optionalAttrs isStandalone {
      # G-Sync/VRR (moved from modules/nvidia.nix)
      __GL_GSYNC_ALLOWED = "1";
      __GL_VRR_ALLOWED = "1";
      # AMD 7950X3D V-Cache core topology for Wine (moved from quirks/amd-x3d.nix)
      WINE_CPU_TOPOLOGY = "16:0,1,2,3,4,5,6,7,16,17,18,19,20,21,22,23";
    };

    # Add directories to PATH
    sessionPath = [
      "/home/svein/.local/bin"
      "/home/svein/.cargo/bin"
      "/home/svein/.npm-global/bin"
    ];

    # Shell aliases
    shellAliases = {
      claude = "~/.local/bin/claude";
      codex = "npx @openai/codex@latest";
      za = "zellij a";
    };
  };

  # Program Configurations
  programs = {
    fish.enable = true;
    fish.shellInit = "
      if [ -e /usr/share/cachyos-fish-config/cachyos-config.fish ]
        source /usr/share/cachyos-fish-config/cachyos-config.fish
      end
    ";
    zsh.enable = true;

    home-manager.enable = isStandalone;

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

  # Agenix secret decryption
  age = {
    identityPaths = [ "${config.home.homeDirectory}/.ssh/id_ed25519" ];
    secrets.magic-reboot-key = {
      file = ../secrets/magic-reboot.key.age;
      path = "${config.home.homeDirectory}/.config/agenix/magic-reboot.key";
    };
    secrets.wireguard-saya = {
      file = ../secrets/wireguard-saya.age;
      path = "${config.home.homeDirectory}/.config/agenix/wireguard-saya";
    };
    secrets.restic-pw = {
      file = ../secrets/restic.pw.age;
      path = "${config.home.homeDirectory}/.config/agenix/restic.pw";
    };
  };

  # Additional packages
  home.packages = [
    magic-reboot
    pkgs.nix-output-monitor
  ] ++ lib.optionals isStandalone ([
    # Custom Rust tools (NixOS machines get these via modules/default.nix)
    ping-discord
    network-monitor
  ] ++ lib.optional (colmenaPackage != null) colmenaPackage);

  # Symlink claude files back to ~/.claude
  home.file = {
    ".claude/CLAUDE.md".text =
      let
        platform =
          if isStandalone then "The machine runs on CachyOS (Arch-based). Use pacman/paru to install packages. nix-shell is also available."
          else if isDarwin then "The machine runs on macOS with nix-darwin."
          else "The machine runs on NixOS. nix-shell is available if a command is missing. If you see 'command not found', try again with nix-shell.";
      in
      ''
        - If there is a battle tested, well known package that can help us, always recommend it. Ask the user's opinion before proceeding.
        - ${platform}
        - When working with rust, look in the rust registry if you need more information on a library.
        - There is project-specific documentation in docs/. Use it when it exists, though bear in mind it may be outdated. Check the 'last updated' tag at the top.
      '';
    ".claude/agents" = {
      source = ../docs/agents;
      recursive = true;
    };
  };


  # Custom terminfo entries (Darwin only - NixOS handles this system-wide)
  home.activation.buildTerminfo = lib.mkIf pkgs.stdenv.isDarwin ''
    mkdir -p $HOME/.terminfo
    ${pkgs.ncurses}/bin/tic -o $HOME/.terminfo ${./terminfo/xterm-ghostty.terminfo}
  '';

  # Do not modify unless you want to delete your home directory.
  home.stateVersion = "25.05";
}
