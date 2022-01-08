{ pkgs, ... }:

{
  imports = [
  ];

  home.username = "svein";
  home.homeDirectory = "/home/svein";

  home.packages = with pkgs; [
    htop fortune mosh
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

  programs.rtorrent = {
    enable = true;
    settings = ''
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

  programs.mpv = {
    enable = true;
    config = {
      ontop = true;
      alang = "ja";
      slang = "en";
      vo = "wlshm";
    };
  };

  programs.neovim = {
    enable = true;
    vimAlias = true;
    extraPython3Packages = (ps: with ps; [
    ]);
    plugins = with pkgs.vimPlugins; [
#    {
#      plugin = vim-plug;
#      config = ''
#        call plug#begin('~/.local/share/nvim/plugged')
#        Plug 'roxma/nvim-yarp'
#        Plug 'ncm2/ncm2-bufword'
#        Plug 'ncm2/ncm2-path'
#        Plug 'neoclide/coc.nvim', {'branch': 'release'}
#        Plug 'LnL7/vim-nix'
#
#        " Writing libs
#        Plug 'tpope/vim-markdown'
#        Plug 'kana/vim-textobj-user'
#        Plug 'reedes/vim-pencil'
#        Plug 'reedes/vim-lexical'
#        Plug 'reedes/vim-litecorrect'
#        Plug 'reedes/vim-textobj-quote'
#        Plug 'reedes/vim-textobj-sentence'
#        Plug 'reedes/vim-wordy'
#        call plug#end()
#      '';}

      # "Defaults everyone can agree on"
      sensible

      # Tools
      fugitive
      The_NERD_tree

      # Syntax support
      syntastic
      #vim-nix
      #rust-vim
      {
        plugin = ncm2;
        config = ''
          " enable ncm2 for all buffers
          autocmd BufEnter * call ncm2#enable_for_buffer()
          set completeopt=noinsert,menuone,noselect
        '';
      }

      # Extra writing tools
      surround
      vim-easymotion
      {
        plugin = vim-pencil;
        config = ''
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
        '';
      } {
        plugin = limelight-vim;
        config = ''
          let g:limelight_conceal_ctermfg = 'gray'
        '';
      }
      airline
      goyo
    ];
    extraConfig = ''
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
