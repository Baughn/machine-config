{ pkgs, ... }:

{
  users.defaultUserShell = pkgs.zsh;

  programs.zsh = {
    enable = true;
    enableCompletion = true;
    autosuggestions.enable = true;
    syntaxHighlighting.enable = true;
    ohMyZsh = {
      enable = true;
      theme = "robbyrussell";
    };

    shellAliases = {
      ll = "ls -l";
    };

    histSize = 100000;
    histFile = "$HOME/.zsh_history";
    setOptions = [
      "HIST_IGNORE_ALL_DUPS"
    ];
  };

  environment.sessionVariables = {
    "EDITOR" = "nvim";
    "LESS" = "FRX";
  };

  programs.tmux.enable = true;
  programs.direnv.enable = true;
  environment.homeBinInPath = true;
  environment.localBinInPath = true;
}
