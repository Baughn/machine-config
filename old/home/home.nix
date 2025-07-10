{ pkgs, ... }: {
  imports = [
  ];

  home.username = "svein";
  home.homeDirectory = "/home/svein";

  home.packages = with pkgs; [
    htop
    fortune
    mosh
  ];

  home.sessionVariables = {
    EDITOR = "vim";
    # Workaround for #599
    LOCALE_ARCHIVE = "${pkgs.glibcLocales}/lib/locale/locale-archive";
    PATH = "$HOME/.cargo/bin:$HOME/bin:$PATH";
    LIBVIRT_DEFAULT_URI = "qemu:///system";
  };

  programs.git = {
    enable = true;
    userName = "Svein Ove Aas";
    userEmail = "sveina@gmail.com";
    lfs.enable = true;
  };

  home.file.".screenrc".text = ''
    defscrollback 5000
    defutf8 on
    vbell off
    maptimeout 5
  '';

  programs.vscode.enable = true;

  programs.rtorrent = {
    enable = true;
    extraConfig = ''
      upload_rate = 1000
      directory = /home/svein/incoming
      session = /home/svein/incoming/.rtorrent
      port_range = 6900-6999
      encryption = allow_incoming,try_outgoing,enable_retry
      dht = on
    '';
  };

  programs.tmux = {
    enable = true;
    escapeTime = 10;
    terminal = "tmux-256color";
  };

  programs.neovim = {
    enable = true;
    vimAlias = true;
    extraPackages = [ pkgs.nodejs ];
  };

  programs.ssh = {
    enable = true;
    compression = false;
    controlMaster = "auto";
    controlPersist = "2m";
    hashKnownHosts = false;
    matchBlocks = {
      "sv" = {
        user = "minecraft";
        hostname = "173.231.55.228";
      };
      "brage.info" = {
        hostname = "10.171.0.1";
      };
    };
    extraConfig = ''
      User svein
    '';
  };

  programs.zsh = {
    enable = true;
    autosuggestion.enable = true;
    enableCompletion = true;
    oh-my-zsh.enable = true;
    oh-my-zsh.plugins = [ "git" "sudo" ];

    oh-my-zsh.theme = "afowler";
    profileExtra = ''
      if [ -e $HOME/.nix-profile/etc/profile.d/nix.sh ]; then . $HOME/.nix-profile/etc/profile.d/nix.sh; fi
    '';
    initContent = ''
      export GOPATH=$HOME/go

      with() {
        local PKG="$1"
        shift
        nix-shell -p "$PKG" --run "$*"
      }
    '';
  };

  programs.home-manager.enable = true;
  home.stateVersion = "21.05";
}
