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

  programs.vscode.enable = true;
  services.vscode-server.enable = true;

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
    plugins = with pkgs.vimPlugins; [
      # "Defaults everyone can agree on"
      sensible

      # Tools
      fugitive
      The_NERD_tree

      # Text objects
      vim-textobj-user

      # Syntax/language support
      syntastic
      vim-nix

      # Rust
      nvim-lspconfig
      nvim-cmp
      cmp-nvim-lsp
      cmp-vsnip
      cmp-path
      cmp-buffer
      rust-tools-nvim
      vim-vsnip
      popup-nvim
      plenary-nvim
      telescope-nvim

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
        " Enable Rust
        autocmd BufReadPost *.rs setlocal filetype=rust

        " Setup LSP
        lua<<EOF
        local nvim_lsp = require'lspconfig'

        local opts = {
            tools = { -- rust-tools options
                autoSetHints = true,
                hover_with_actions = true,
                inlay_hints = {
                    show_parameter_hints = false,
                    parameter_hints_prefix = "",
                    other_hints_prefix = "",
                },
            },

            -- all the opts to send to nvim-lspconfig
            -- these override the defaults set by rust-tools.nvim
            -- see https://github.com/neovim/nvim-lspconfig/blob/master/doc/server_configurations.md#rust_analyzer
            server = {
                -- on_attach is a callback called when the language server attachs to the buffer
                -- on_attach = on_attach,
                settings = {
                    -- to enable rust-analyzer settings visit:
                    -- https://github.com/rust-analyzer/rust-analyzer/blob/master/docs/user/generated_config.adoc
                    ["rust-analyzer"] = {
                        -- enable clippy on save
                        checkOnSave = {
                            command = "clippy"
                        },
                    }
                }
            },
        }
        require('rust-tools').setup(opts)
        EOF


        " Setup Completion
        " See https://github.com/hrsh7th/nvim-cmp#basic-configuration
        lua <<EOF
        local cmp = require'cmp'
        cmp.setup({
          -- Enable LSP snippets
          snippet = {
            expand = function(args)
                vim.fn["vsnip#anonymous"](args.body)
            end,
          },
          mapping = {
            ['<C-p>'] = cmp.mapping.select_prev_item(),
            ['<C-n>'] = cmp.mapping.select_next_item(),
            -- Add tab support
            ['<S-Tab>'] = cmp.mapping.select_prev_item(),
            ['<Tab>'] = cmp.mapping.select_next_item(),
            ['<C-d>'] = cmp.mapping.scroll_docs(-4),
            ['<C-f>'] = cmp.mapping.scroll_docs(4),
            ['<C-Space>'] = cmp.mapping.complete(),
            ['<C-e>'] = cmp.mapping.close(),
            ['<CR>'] = cmp.mapping.confirm({
              behavior = cmp.ConfirmBehavior.Insert,
              select = true,
            })
          },

          -- Installed sources
          sources = {
            { name = 'nvim_lsp' },
            { name = 'vsnip' },
            { name = 'path' },
            { name = 'buffer' },
          },
        })
        EOF

        " Required for operations modifying multiple buffers like rename.
        set hidden

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

        set signcolumn=yes

        " Keybindings
        nnoremap <silent> <Space> <cmd>lua vim.lsp.buf.code_action()<CR>
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
