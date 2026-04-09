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

    interactiveShellInit = ''
      _flake_age_warning() {
        local lockfile="$HOME/cachy-nix/flake.lock"
        if [[ -f "$lockfile" ]]; then
          local age=$(( ($(date +%s) - $(stat -c %Y "$lockfile")) / 86400 ))
          if (( age >= 4 )); then
            echo -n "%F{yellow}[flake ''${age}d old]%f "
          fi
        fi
      }
      PROMPT='$(_flake_age_warning)'"$PROMPT"
    '';

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
