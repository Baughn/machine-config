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
  ];

  home.username = "svein";
  home.homeDirectory = "/home/svein";

  home.packages = with pkgs; [
    htop fortune ix mosh
    (callPackage ../tools/up {})
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
  };

  home.file.".screenrc".text = ''
    defscrollback 5000
    defutf8 on
    vbell off
    maptimeout 5
  '';

  programs.tmux = {
    enable = true;
    escapeTime = 10;
    terminal = "tmux-256color";
  };

  programs.mpv = {
    enable = true;
    config = {
      ontop = true;
      alang = "ja";
      slang = "en";
    };
  };

  programs.neovim = {
    enable = true;
    vimAlias = true;
    extraPython3Packages = (ps: with ps; [
      python-language-server
    ]);
    plugins = with pkgs.vimPlugins; [
      # "Defaults everyone can agree on"
      sensible

      # Tools
      fugitive
      The_NERD_tree

      # Syntax support
      syntastic
      #vim-nix
      #rust-vim

      # Extra writing tools
      surround
      vim-easymotion

      # Writing / appearance
      airline
      goyo
      limelight-vim
    ];
    extraConfig = ''
        call plug#begin('~/.local/share/nvim/plugged')
        Plug 'ncm2/ncm2'
        Plug 'roxma/nvim-yarp'
        Plug 'ncm2/ncm2-bufword'
        Plug 'ncm2/ncm2-path'
        Plug 'neoclide/coc.nvim', {'branch': 'release'}

        " Writing libs
        Plug 'tpope/vim-markdown'
				Plug 'kana/vim-textobj-user'
				Plug 'reedes/vim-pencil'
        Plug 'reedes/vim-lexical'
        Plug 'reedes/vim-litecorrect'
        Plug 'reedes/vim-textobj-quote'
        Plug 'reedes/vim-textobj-sentence'
        Plug 'reedes/vim-wordy'
        call plug#end()

        " Writing stuff
        let g:limelight_conceal_ctermfg = 'gray'
				augroup pencil
					 autocmd!
					 autocmd filetype markdown,mkd call pencil#init()
							 \ | call textobj#sentence#init()
							 \ | call textobj#quote#init()
							 \ | call lexical#init()
							 \ | call litecorrect#init()
               \ | Wordy weak
							 \ | setl spell spl=en_us fdl=4 noru nonu nornu
							 \ | setl fdo+=search
					augroup END
				 " Pencil / Writing Controls {{{
					 let g:pencil#wrapModeDefault = 'soft'
					 let g:pencil#textwidth = 74
					 let g:pencil#joinspaces = 0
					 let g:pencil#cursorwrap = 1
					 let g:pencil#conceallevel = 3
					 let g:pencil#concealcursor = 'c'
					 let g:pencil#softDetectSample = 20
					 let g:pencil#softDetectThreshold = 130
				 " }}}

        " enable ncm2 for all buffers
        autocmd BufEnter * call ncm2#enable_for_buffer()
        set completeopt=noinsert,menuone,noselect

        " Use <TAB> to select the popup menu:
        inoremap <expr> <Tab> pumvisible() ? "\<C-n>" : "\<Tab>"
        inoremap <expr> <S-Tab> pumvisible() ? "\<C-p>" : "\<S-Tab>"

        " Enable Rust
        autocmd BufReadPost *.rs setlocal filetype=rust

        " Required for operations modifying multiple buffers like rename.
        set hidden

        nnoremap <silent> K :call LanguageClient_textDocument_hover()<CR>
        nnoremap <silent> gd :call LanguageClient_textDocument_definition()<CR>
        nnoremap <silent> <F2> :call LanguageClient_textDocument_rename()<CR>

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
    '';
  };

  programs.ssh = {
    enable = true;
    compression = true;
    controlMaster = "auto";
    controlPersist = "2m";
    matchBlocks = {
      "*.sv" = {
        identityFile = "/home/svein/sufficient/id_rsa";
        user = "baughn";
      };
    } // (if builtins.readFile /etc/hostname == "tsugumi\n" then {} else {
      "saya" = {
        proxyJump = "brage.info";
      };
    });
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
    oh-my-zsh.theme = "afowler";
    profileExtra = ''
      if [ -e $HOME/.nix-profile/etc/profile.d/nix.sh ]; then . $HOME/.nix-profile/etc/profile.d/nix.sh; fi
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
