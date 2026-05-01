{ pkgs, ... }:

{
  home-manager.useGlobalPkgs = true;
  home-manager.useUserPackages = true;
  home-manager.users.svein = { ... }: {
    home.stateVersion = "24.11";

    home.sessionVariables = {
      NPM_CONFIG_PREFIX = "$HOME/.npm-global";
    };
    home.sessionPath = [ "$HOME/.npm-global/bin" ];

    home.packages = [ pkgs.nodejs ];

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
