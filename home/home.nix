{ pkgs, ... }:

let ix = with pkgs; stdenv.mkDerivation {
  name = "ix";
  src = fetchurl {
    url = "ix.io/client";
    sha256 = "0xc2s4s1aq143zz8lgkq5k25dpf049dw253qxiav5k7d7qvzzy57";
  };
  unpackPhase = "true";
  installPhase = ''
    install -D $src $out/bin/ix
  '';
};
in

{
  home.packages = with pkgs; [
    htop fortune most ix mosh
    (callPackage ../tools/up {})
  ];

  home.sessionVariables = {
    PAGER = "most";
    EDITOR = "vim";
    # Workaround for #599
    LOCALE_ARCHIVE = "${pkgs.glibcLocales}/lib/locale/locale-archive";
  };

  programs.git = {
    enable = true;
    userName = "Svein Ove Aas";
    userEmail = "sveina@gmail.com";
  };

  programs.neovim = {
    enable = true;
    vimAlias = true;
    configure = {
      customRC = ''
        set nocompatible
        filetype on
        filetype indent on
        syntax on

        set tabstop=2
        set shiftwidth=2
        set expandtab
        set smartindent
        set autoindent

        set hlsearch

        set guicursor=

      '';
      packages.myVimPackage = with pkgs.vimPlugins; {
        start = [ fugitive syntastic vim-nix ];
	opt = [];
      };
    };
  };

  programs.ssh = {
    enable = true;
    compression = true;
    controlMaster = "auto";
    controlPersist = "10m";
    matchBlocks = {
      "saya" = {
        hostname = "brage.info";
        port = 2222;
      };
    };
    extraConfig = ''
      User svein
    '';
  };

  programs.zsh = {
    enable = true;
    enableAutosuggestions = true;
    enableCompletion = true;
    oh-my-zsh.enable = true;
    oh-my-zsh.plugins = [ "git" "sudo" ];
    oh-my-zsh.theme = "robbyrussell";
    profileExtra = ''
      if [ -e $HOME/.nix-profile/etc/profile.d/nix.sh ]; then . $HOME/.nix-profile/etc/profile.d/nix.sh; fi
    '';
  };


  programs.home-manager.enable = true;
  programs.home-manager.path = https://github.com/rycee/home-manager/archive/master.tar.gz;
}
