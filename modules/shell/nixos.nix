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
      _failed_units_warning() {
        local parts=() n
        if [[ "$(systemctl is-system-running 2>/dev/null)" == "degraded" ]]; then
          n=$(systemctl list-units --state=failed --no-legend --plain 2>/dev/null | wc -l)
          (( n > 0 )) && parts+=("''${n} sys")
        fi
        if [[ "$(systemctl --user is-system-running 2>/dev/null)" == "degraded" ]]; then
          n=$(systemctl --user list-units --state=failed --no-legend --plain 2>/dev/null \
                | grep -Ev '^app-steam[@-]' | wc -l)
          (( n > 0 )) && parts+=("''${n} usr")
        fi
        if (( ''${#parts[@]} > 0 )); then
          echo -n "%F{red}[failed: ''${(j:, :)parts}]%f "
        fi
      }
      # oh-my-zsh sets PROMPT when sourced later in /etc/zshrc, so we
      # can't prepend here directly. Defer the prepend to the first
      # precmd, by which point the theme has loaded.
      _prompt_warnings_install() {
        PROMPT='$(_failed_units_warning)$(_flake_age_warning)'"$PROMPT"
        precmd_functions=(''${precmd_functions:#_prompt_warnings_install})
      }
      precmd_functions+=(_prompt_warnings_install)
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
