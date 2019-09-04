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
    PATH = "$PATH:$HOME/.cargo/bin";
  };

  programs.git = {
    enable = true;
    userName = "Svein Ove Aas";
    userEmail = "sveina@gmail.com";
  };

  home.file.".screenrc".text = ''
    defscrollback 5000
    defutf8 on
    vbell off
    maptimeout 5
  '';

  programs.neovim = {
    enable = true;
    vimAlias = true;
    extraPython3Packages = (ps: with ps; [ python-language-server ]);
    plugins = with pkgs.vimPlugins; [
      # "Defaults everyone can agree on"
      sensible

      # Syntax support
      syntastic
      vim-nix
      rust-vim

      ## Plugins
      ''
        call plug#begin('~/.local/share/nvim/plugged')
        Plug 'autozimu/LanguageClient-neovim', { 'do': ':UpdateRemotePlugins' }
        Plug 'junegunn/fzf'
        Plug 'ncm2/ncm2'
        Plug 'roxma/nvim-yarp'
        Plug 'ncm2/ncm2-bufword'
        Plug 'ncm2/ncm2-path'
        call plug#end()

        autocmd BufReadPost *.rs setlocal filetype=rust
        autocmd BufEnter * call ncm2#enable_for_buffer()
        " IMPORTANT: :help Ncm2PopupOpen for more information
        set completeopt=noinsert,menuone,noselect

        " Required for operations modifying multiple buffers like rename.
        set hidden

        let g:LanguageClient_serverCommands = {
            \ 'rust': ['rustup', 'run', 'stable', 'rls'],
            \ }

        " Automatically start language servers.
        let g:LanguageClient_autoStart = 1

        let g:rustfmt_command = "rustfmt +nightly"
        let g:rustfmt_emit_files = 1
        let g:rustfmt_autosave = 1

        " Maps K to hover, gd to goto definition, F2 to rename, F5 to context menu.
        nnoremap <silent> K :call LanguageClient_textDocument_hover()<CR>
        nnoremap <silent> gd :call LanguageClient_textDocument_definition()<CR>
        nnoremap <silent> <F2> :call LanguageClient_textDocument_rename()<CR>
        nnoremap <F5> :call LanguageClient_contextMenu()<CR>

        set completeopt+=preview
      ''

      # Writing
      goyo
      #limelight

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

        " Splits
        set splitbelow
        set splitright

        set timeoutlen=100 ttimeoutlen=10
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
      "*.sv" = {
        identityFile = "/home/svein/sufficient/id_rsa";
        user = "baughn";
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
