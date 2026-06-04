{ pkgs, agenix, ... }:

{
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.backupFileExtension = "hm-backup";
  home-manager.extraSpecialArgs = { inherit agenix; };
  home-manager.users.svein = { agenix, ... }: {
    imports = [ agenix.homeManagerModules.default ];

    home.stateVersion = "24.11";

    home.sessionVariables = {
      NPM_CONFIG_PREFIX = "$HOME/.npm-global";
    };
    home.sessionPath = [ "$HOME/.npm-global/bin" ];

    home.packages = [ pkgs.nodejs ];

    programs.neovim = {
      enable = true;
      withPython3 = false;
      withRuby = false;
      plugins = [ pkgs.vimPlugins.nvim-treesitter.withAllGrammars ];
      extraConfig = ''
        set expandtab
        set tabstop=2
        set softtabstop=2
        set shiftwidth=2
      '';
      initLua = ''
        -- Start Treesitter highlighting (and language injections) for any
        -- buffer whose filetype has a parser. pcall: silently no-op otherwise.
        vim.api.nvim_create_autocmd('FileType', {
          callback = function() pcall(vim.treesitter.start) end,
        })
      '';
    };

    programs.jujutsu = {
      enable = true;
      package = null;  # jujutsu is installed system-wide via cli-tools
      settings = {
        ui = {
          default-command = "log";
          diff-formatter = [ "difft" "--color=always" "$left" "$right" ];
          merge-editor = "mergiraf";
        };
        user = {
          name = "Svein Ove Aas";
          email = "sveina@gmail.com";
        };
      };
    };

    # Force grayscale antialiasing. saya's monitors have mismatched subpixel
    # layouts (one IPS RGB-stripe, three QD-OLED with triangular subpixels, one
    # of them rotated to portrait), so no single subpixel order is correct.
    # Subpixel rendering produced color fringing (thin black lines turning red)
    # in fontconfig-driven apps like Chrome. "none" => rgba=none via mode=assign,
    # which overrides any stale/DE-injected value. See also the system-level
    # fonts.fontconfig.subpixel.rgba in machines/saya/default.nix.
    fonts.fontconfig = {
      antialiasing = true;
      hinting = "slight";
      subpixelRendering = "none";
    };

    home.file = {
      ".claude/CLAUDE.md".text = ''
        - If there is a battle tested, well known package that can help us, you can recommend it. Ask the user's opinion before proceeding.
        - This machine runs on NixOS. If you see 'command not found', try again with `nix run nixpkgs#package -- args`.
        - There may be project-specific documentation in docs/. Use it when it exists, though bear in mind it may be outdated. Check the 'last updated' tag at the top.
	- The user uses Jujutsu. Prioritize jj commands over git.
      '';
    };

    programs.home-manager.enable = true;
  };
}
