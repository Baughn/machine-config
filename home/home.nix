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
  imports = [
    ./neovim-plugins.nix
  ];

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
    plugins = with pkgs.vimPlugins; [
      # "Defaults everyone can agree on"
      sensible

      # Syntax support
      syntastic
      vim-nix
      rust-vim
      ''
        let g:rustfmt_command = "rustfmt +nightly"
        let g:rustfmt_emit_files = 1
        let g:rustfmt_autosave = 1
      ''

      # Personal customizations
      ''
        set nocompatible
        
        set tabstop=2
        set shiftwidth=2
        set expandtab
        set smartindent
        set autoindent

        set hlsearch

        set guicursor=

        colorscheme desert
      ''
    ];
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
    oh-my-zsh.theme = "af-magic";
    profileExtra = ''
      if [ -e $HOME/.nix-profile/etc/profile.d/nix.sh ]; then . $HOME/.nix-profile/etc/profile.d/nix.sh; fi
      export GOPATH=$HOME/go
    '';
  };


  programs.home-manager.enable = true;
  programs.home-manager.path = https://github.com/rycee/home-manager/archive/release-19.03.tar.gz;
}
