{ config
, lib
, pkgs
, ...
}:
with lib;
# See: https://nixos.org/nixos/manual/index.html#sec-writing-modules
let
  cfg = config.programs.neovim.plugins;
  oneOf = typeList: foldList (types.either) typeList;
  foldList = f: list: builtins.foldl' f (builtins.head list) (builtins.tail list);
  filterMap = f: list:
    builtins.filter (x: x != null) (map f list);

  pluginFromGitHubURL = url:
    let
      matches = builtins.match "https://github[.]com/([^/]+)/([^/]+)/archive/(.+)\\.tar\\.gz#?([a-z0-9]*)" url;
      parts =
        if matches == null
        then null
        else {
          owner = builtins.elemAt matches 0;
          repo = builtins.elemAt matches 1;
          rev = builtins.elemAt matches 2;
          sha256 = builtins.elemAt matches 3;
        };
    in
    if parts == null
    then null
    else
      pkgs.vimUtils.buildVimPluginFrom2Nix {
        name = "${parts.owner}-${parts.repo}";
        src = pkgs.fetchFromGitHub parts;
      };

  isSimpleString = x:
    (builtins.isString x) && ((pluginFromGitHubURL x) == null);

  toVamPlugin = x:
    if builtins.isAttrs x
    then x
    else pluginFromGitHubURL x;

  loadPlugin = plugin: ''
    set rtp^=${plugin.rtp}
    set rtp+=${plugin.rtp}/after
  '';
in
{
  # Interface
  options = {
    programs.neovim.plugins = mkOption {
      type = types.listOf (oneOf [ types.str types.package ]);
      default = [ ];
      description = ''
        A list of neovim plugins. Elements can be:
        - Vam plugins - those will be installed in neovim; example:
            pkgs.vimPlugins.fugitive
        - GitHub archive URLs - those will be treated as plugin URLs, downloaded and
          installed in neovim. The URLs must have a # and sha256 suffix, which can be
          found by running `nix-prefetch-url --unpack <GitHub-URL>`. Example:
            https://github.com/bkad/CamelCaseMotion/archive/3ae9bf93cce28ddc1f2776999ad516e153769ea4.tar.gz#086q1n0d8xaa0nxvwxlhpwi1hbdz86iaskbl639z788z0ysh4dxw
        - non-URL strings - those will be copied verbatim to vim config (.vimrc); example:
            '''
              " bash-like (or, readline-like) tab completion of paths, case insensitive
              set wildmode=longest,list,full
              set wildmenu
              if exists("&wildignorecase")
                set wildignorecase
              endif
            '''
      '';
    };
  };

  # Implementation
  config = {
    # Note: see `man home-configuration.nix` -> programs.neovim.configure
    programs.neovim.configure.customRC = ''
      " Workaround for broken handling of packpath by vim8/neovim for ftplugins
      filetype off | syn off
      ${builtins.concatStringsSep "\n"
        (map loadPlugin
          (filterMap toVamPlugin cfg))}
      filetype indent plugin on | syn on

      ${builtins.concatStringsSep "\n" (builtins.filter isSimpleString cfg)}
    '';
  };
}
