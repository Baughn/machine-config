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
    htop fortune ix mosh
    (callPackage ../tools/up {})
  ];

  home.sessionVariables = {
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

      # LSP
      "https://github.com/prabirshrestha/async.vim/archive/f3014550d7a799097e56b094104dd2cd66cf2612.tar.gz#0zn25qwycynagrij5rsp1x7kbfz612gn7xda0hvm4y7qr3pal77p"
      "https://github.com/prabirshrestha/vim-lsp/archive/69f31d5bf27eac0ef4b804d0e517d6e85856b44a.tar.gz#1wic4bpddzbbnkd1jfirb4l10jynz3cj2y0d2q23xkj9f56q9l53"
      "https://github.com/prabirshrestha/asyncomplete.vim/archive/bffa8b62dd7025f400891182136148773d42f075.tar.gz#1nl402qqp88p7zbm4k9b7fzyckrxjkh47iqwrin2lkqk6bhmc690"
      "https://github.com/prabirshrestha/asyncomplete-lsp.vim/archive/05389e93a81aa4262355452ebdac66ae2a1939fb.tar.gz#0mnsp54p0i6x7w1zlmwhpi2hhwb787z1p69pr2lmz7qja2iqv36y"

      ## Rust
      ''
        if executable('rls')
          au User lsp_setup call lsp#register_server({
            \ 'name': 'rls',
            \ 'cmd': {server_info->['rustup', 'run', 'nightly', 'rls']},
            \ 'whitelist': ['rust'],
            \ })
        endif 
      ''

      # Writing
      goyo
      #limelight
      ''
        let g:rustfmt_command = "rustfmt +nightly"
        let g:rustfmt_emit_files = 1
        let g:rustfmt_autosave = 1
      ''

      # Personal customizations
      ''
        set nocompatible
        set linebreak
        
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
