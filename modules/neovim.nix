{ config
, pkgs
, lib
, ...
}:
let
  cfg = config.me.neovim;
in
{
  options.me.neovim = {
    enable = lib.mkEnableOption "Enhanced Neovim configuration with LSP and Copilot";

    enableCopilot = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable GitHub Copilot integration";
    };

    languages = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "nix" "python" "rust" "typescript" "javascript" ];
      description = "List of languages to configure LSP support for";
    };
  };

  config = lib.mkIf cfg.enable {
    programs.neovim = {
      enable = true;
      defaultEditor = true;
      vimAlias = true;
      viAlias = true;
      withNodeJs = true;
      withPython3 = true;

      configure = {
        customRC = ''
          " Set leader key
          let mapleader = " "
          let maplocalleader = " "
          
          " Basic settings
          set number relativenumber
          set mouse=a
          set ignorecase smartcase
          set nohlsearch
          set nowrap
          set breakindent
          set tabstop=2 shiftwidth=2 expandtab
          set termguicolors
          set signcolumn=yes
          set updatetime=250
          set timeoutlen=300
          set completeopt=menuone,noselect
          set undofile
          set splitbelow splitright
          set clipboard=unnamedplus
          
          " Theme
          colorscheme catppuccin-mocha
          
          " File explorer
          nnoremap <C-n> :NvimTreeToggle<CR>
          
          " Telescope mappings
          nnoremap <leader>ff <cmd>Telescope find_files<cr>
          nnoremap <leader>fg <cmd>Telescope live_grep<cr>
          nnoremap <leader>fb <cmd>Telescope buffers<cr>
          nnoremap <leader>fh <cmd>Telescope help_tags<cr>
        '' + lib.optionalString cfg.enableCopilot ''
          
          " Copilot settings
          let g:copilot_no_tab_map = v:true
          imap <silent><script><expr> <C-J> copilot#Accept("\<CR>")
          
          " Optional: Disable copilot for certain filetypes
          let g:copilot_filetypes = {
            \ 'gitcommit': v:false,
            \ 'markdown': v:true,
            \ 'yaml': v:true,
            \ }
        '';

        customLuaRC = ''
                    -- Highlight on yank
                    local highlight_group = vim.api.nvim_create_augroup('YankHighlight', { clear = true })
                    vim.api.nvim_create_autocmd('TextYankPost', {
                      callback = function()
                        vim.highlight.on_yank()
                      end,
                      group = highlight_group,
                      pattern = '*',
                    })
          
                    -- Set up diagnostics
                    vim.diagnostic.config({
                      virtual_text = true,
                      signs = true,
                      underline = true,
                      update_in_insert = false,
                      severity_sort = true,
                    })
          
                    -- Configure nvim-tree
                    require("nvim-tree").setup({
                      view = {
                        width = 30,
                      },
                      renderer = {
                        group_empty = true,
                      },
                      filters = {
                        dotfiles = false,
                      },
                    })
          
                    -- Configure lualine
                    require('lualine').setup {
                      options = {
                        theme = 'catppuccin',
                        icons_enabled = true,
                      },
                      sections = {
                        lualine_x = { 'copilot', 'encoding', 'fileformat', 'filetype' },
                      }
                    }
          
                    -- Configure telescope
                    require('telescope').setup{
                      defaults = {
                        mappings = {
                          i = {
                            ["<C-u>"] = false,
                            ["<C-d>"] = false,
                          },
                        },
                      },
                    }
          
                    -- LSP Configuration
                    -- Global mappings
                    vim.keymap.set('n', '<space>e', vim.diagnostic.open_float)
                    vim.keymap.set('n', '[d', vim.diagnostic.goto_prev)
                    vim.keymap.set('n', ']d', vim.diagnostic.goto_next)
                    vim.keymap.set('n', '<space>q', vim.diagnostic.setloclist)

                    -- Use LspAttach autocommand to only map the following keys
                    -- after the language server attaches to the current buffer
                    vim.api.nvim_create_autocmd('LspAttach', {
                      group = vim.api.nvim_create_augroup('UserLspConfig', {}),
                      callback = function(ev)
                        -- Enable completion triggered by <c-x><c-o>
                        vim.bo[ev.buf].omnifunc = 'v:lua.vim.lsp.omnifunc'

                        -- Buffer local mappings
                        local opts = { buffer = ev.buf }
                        vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, opts)
                        vim.keymap.set('n', 'gd', vim.lsp.buf.definition, opts)
                        vim.keymap.set('n', 'K', vim.lsp.buf.hover, opts)
                        vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, opts)
                        vim.keymap.set('n', '<C-k>', vim.lsp.buf.signature_help, opts)
                        vim.keymap.set('n', '<space>wa', vim.lsp.buf.add_workspace_folder, opts)
                        vim.keymap.set('n', '<space>wr', vim.lsp.buf.remove_workspace_folder, opts)
                        vim.keymap.set('n', '<space>wl', function()
                          print(vim.inspect(vim.lsp.buf.list_workspace_folders()))
                        end, opts)
                        vim.keymap.set('n', '<space>D', vim.lsp.buf.type_definition, opts)
                        vim.keymap.set('n', '<space>rn', vim.lsp.buf.rename, opts)
                        vim.keymap.set({ 'n', 'v' }, '<space>ca', vim.lsp.buf.code_action, opts)
                        vim.keymap.set('n', 'gr', vim.lsp.buf.references, opts)
                        vim.keymap.set('n', '<space>f', function()
                          vim.lsp.buf.format { async = true }
                        end, opts)
                      end,
                    })

                    -- Configure LSP servers using new vim.lsp.config API
                    vim.lsp.config.nil_ls = {
                      cmd = { 'nil' },
                      filetypes = { 'nix' },
                      root_markers = { 'flake.nix', '.git' },
                      settings = {
                        ['nil'] = {
          	        nix = {
          	          flake = {
                              autoArchive = false,
                              autoEvalInputs = false,
          		  },
          		},
                        },
                      },
                    }

                    vim.lsp.config.rust_analyzer = {
                      cmd = { 'rust-analyzer' },
                      filetypes = { 'rust' },
                      root_markers = { 'Cargo.toml' },
                    }

                    vim.lsp.config.ts_ls = {
                      cmd = { 'typescript-language-server', '--stdio' },
                      filetypes = { 'javascript', 'javascriptreact', 'typescript', 'typescriptreact' },
                      root_markers = { 'package.json', 'tsconfig.json', 'jsconfig.json' },
                    }

                    vim.lsp.config.pylsp = {
                      cmd = { 'pylsp' },
                      filetypes = { 'python' },
                      root_markers = { 'pyproject.toml', 'setup.py', 'setup.cfg', 'requirements.txt', 'Pipfile' },
                    }

                    vim.lsp.config.lua_ls = {
                      cmd = { 'lua-language-server' },
                      filetypes = { 'lua' },
                      root_markers = { '.luarc.json', '.luarc.jsonc', '.luacheckrc', '.stylua.toml', 'stylua.toml', 'selene.toml', 'selene.yml' },
                      settings = {
                        Lua = {
                          runtime = {
                            version = 'LuaJIT',
                          },
                          diagnostics = {
                            globals = {'vim'},
                          },
                          workspace = {
                            library = vim.api.nvim_get_runtime_file("", true),
                            checkThirdParty = false,
                          },
                          telemetry = {
                            enable = false,
                          },
                        },
                      },
                    }

                    -- Enable LSP servers
                    vim.lsp.enable('nil_ls')
                    vim.lsp.enable('rust_analyzer')
                    vim.lsp.enable('ts_ls')
                    vim.lsp.enable('pylsp')
                    vim.lsp.enable('lua_ls')
          
                    -- Configure nvim-cmp
                    local cmp = require'cmp'
          
                    cmp.setup({
                      snippet = {
                        expand = function(args)
                          vim.fn["vsnip#anonymous"](args.body)
                        end,
                      },
                      mapping = cmp.mapping.preset.insert({
                        ['<C-b>'] = cmp.mapping.scroll_docs(-4),
                        ['<C-f>'] = cmp.mapping.scroll_docs(4),
                        ['<C-Space>'] = cmp.mapping.complete(),
                        ['<C-e>'] = cmp.mapping.abort(),
                        ['<CR>'] = cmp.mapping.confirm({ select = true }),
                      }),
                      sources = cmp.config.sources({
                        { name = 'nvim_lsp' },
                        { name = 'vsnip' },
                        { name = 'copilot' },
                      }, {
                        { name = 'buffer' },
                      })
                    })
          
                    -- Configure treesitter
                    require'nvim-treesitter.configs'.setup {
                      highlight = {
                        enable = true,
                        additional_vim_regex_highlighting = false,
                      },
                      indent = {
                        enable = true,
                      },
                    }
          
                    -- Configure gitsigns
                    require('gitsigns').setup()
          
                    -- Configure comment.nvim
                    require('Comment').setup()
          
                    -- Configure nvim-surround
                    require("nvim-surround").setup()
          
                    -- Configure nvim-autopairs
                    require('nvim-autopairs').setup{}
          
                    -- Configure which-key
                    require("which-key").setup{}
        '';

        packages.myVimPackage = with pkgs.vimPlugins; {
          start = [
            # Theme
            catppuccin-nvim

            # Core plugins
            plenary-nvim
            nvim-web-devicons

            # File explorer
            nvim-tree-lua

            # Status line
            lualine-nvim

            # Fuzzy finder
            telescope-nvim

            # Autocompletion
            nvim-cmp
            cmp-nvim-lsp
            cmp-buffer
            cmp-vsnip
            vim-vsnip

            # Treesitter
            nvim-treesitter.withAllGrammars

            # Git
            gitsigns-nvim
            vim-fugitive

            # Utils
            comment-nvim
            nvim-surround
            nvim-autopairs
            which-key-nvim
          ] ++ lib.optionals cfg.enableCopilot [
            copilot-vim
          ];
        };
      };
    };

    # Extra packages for LSP servers and tools
    environment.systemPackages = with pkgs; [
      # LSP servers
      nil
      rust-analyzer
      nodePackages.typescript-language-server
      nodePackages.vscode-langservers-extracted
      python3Packages.python-lsp-server
      lua-language-server

      # Formatters and linters
      nixpkgs-fmt
      rustfmt
      nodePackages.prettier
      python3Packages.black
      python3Packages.isort

      # Other tools
      ripgrep
      fd
      tree-sitter
    ];
  };
}
