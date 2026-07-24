{ agenix, ... }:

{
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "hm-backup";

  home-manager.users.svein = { config, lib, pkgs, ... }:
    let
      magic-reboot-send = pkgs.callPackage ../../tools/magic-reboot/sender { };

      # Wrapper that auto-injects the decrypted key path
      magic-reboot = pkgs.writeShellScriptBin "magic-reboot" ''
        exec ${magic-reboot-send}/bin/magic-reboot-send --key ${config.age.secrets.magic-reboot-key.path} "$@"
      '';

      sunakuCustomTheme = pkgs.callPackage ./zsh-sunaku-theme.nix { };
    in
    {
      imports = [ agenix.homeManagerModules.default ];

      home.sessionVariables = {
        EDITOR = "nvim";
        # Claude
        BASH_DEFAULT_TIMEOUT_MS = 300000;
        BASH_MAX_TIMEOUT_MS = 1800000;
      };

      home.sessionPath = [
        "$HOME/.local/bin"
        "$HOME/.cargo/bin"
        "$HOME/.npm-global/bin"
      ];

      home.shellAliases = {
        claude = "~/.local/bin/claude";
        codex = "npx @openai/codex@latest";
        za = "zellij a";
      };

      programs.zsh = {
        enable = true;
        autosuggestion.enable = true;
        oh-my-zsh = {
          enable = true;
          theme = "sunaku-custom";
          custom = "${sunakuCustomTheme}/share/zsh";
          plugins = [
            "sudo"
            "git"
            "jj"
            "ssh"
          ];
        };
      };

      programs.git = {
        enable = true;
        settings.user = {
          name = "Svein Ove Aas";
          email = "sveina@gmail.com";
        };
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

      # The v4 IPv4-proxy fallbacks and check-ipv6 matching that used to live
      # here died with the v4 machine; these are plain direct connections now.
      programs.ssh = {
        enable = true;
        enableDefaultConfig = false;
        extraConfig = ''
          ConnectTimeout 30
          TCPKeepAlive yes
          ConnectionAttempts 2
        '';
        matchBlocks = {
          "*" = {
            controlMaster = "auto";
            controlPath = "~/.ssh/control-%r@%h:%p";
            controlPersist = "10m";
            serverAliveInterval = 60;
            serverAliveCountMax = 3;
          };
          "brage.info tsugumi direct.brage.info" = {
            hostname = "direct.brage.info";
            port = 22;
          };
          "saya saya.brage.info" = {
            hostname = "saya.brage.info";
            port = 22;
          };
        };
      };

      programs.rtorrent = {
        enable = true;
        extraConfig = ''
          upload_rate = 204800
          download_rate = 2097152
          directory.default.set = ~/Downloads
          session.path.set = ~/.rtorrent
          protocol.encryption.set = allow_incoming,try_outgoing,enable_retry
        '';
      };

      programs.direnv = {
        enable = true;
        enableZshIntegration = true;
        nix-direnv.enable = true;
      };

      # Agenix secret decryption
      age = {
        identityPaths = [ "${config.home.homeDirectory}/.ssh/id_ed25519" ];
        secrets.magic-reboot-key.file = ../../secrets/magic-reboot.key.age;
      };

      home.packages = [ magic-reboot ];

      home.file = {
        ".claude/CLAUDE.md".source = ./claude/CLAUDE.md;
        ".claude/agents" = {
          source = ./claude/agents;
          recursive = true;
        };
      };

      # Custom terminfo entries (NixOS handles this system-wide instead)
      home.activation.buildTerminfo = ''
        mkdir -p $HOME/.terminfo
        ${pkgs.ncurses}/bin/tic -o $HOME/.terminfo ${./terminfo/xterm-ghostty.terminfo}
      '';

      # Do not modify unless you want to delete your home directory.
      home.stateVersion = "25.05";

      programs.home-manager.enable = true;
    };
}
