{ config, lib, pkgs, flakeSelf, ... }:

let
  cfg = config.me.shell;
  userFailedFilter =
    if cfg.userFailedUnitsExclude == []
    then "cat"
    else "grep -Ev '${lib.concatStringsSep "|" cfg.userFailedUnitsExclude}'";
in
{
  options.me.shell.userFailedUnitsExclude = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [];
    description = ''
      Extended-regex patterns matching user systemd units that should NOT count toward
      the prompt's failed-units warning. Patterns are OR-joined into a single grep -Ev.
    '';
    example = [ "^app-steam[@-]" ];
  };

  config = {
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
        _root_indicator() {
          (( EUID == 0 )) && echo -n "%F{red} ROOT%f "
        }
        _flake_age_warning() {
          local last
          [[ -r /etc/nixpkgs-last-modified ]] || return
          last=$(< /etc/nixpkgs-last-modified)
          local age=$(( ($(date +%s) - last) / 86400 ))
          if (( age >= 7 )); then
            echo -n "%F{yellow}[flake ''${age}d old]%f "
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
                  | ${userFailedFilter} | wc -l)
            (( n > 0 )) && parts+=("''${n} usr")
          fi
          if (( ''${#parts[@]} > 0 )); then
            echo -n "%F{red}[failed: ''${(j:, :)parts}]%f "
          fi
        }
        _jj_prompt_info() {
          command -v jj >/dev/null 2>&1 || return
          jj root --quiet >/dev/null 2>&1 || return

          local raw description state
          raw=$(jj log --no-graph --color never -r @ \
            -T 'description.first_line() ++ "\n" ++ if(empty, "empty", "dirty")' \
            2>/dev/null) || return

          description="''${raw%%$'\n'*}"
          state="''${raw##*$'\n'}"

          local -a words
          words=( ''${=description} )

          local name dirty
          if [[ -z "$description" || "$description" == "no description set" ]]; then
            if [[ "$state" == "empty" ]]; then
              name="(empty)"
            else
              name="(no description)"
            fi
          else
            name="''${(j: :)words[1,4]}"
          fi
          [[ "$state" == "dirty" ]] && dirty=" %{$fg[yellow]%}*"

          echo -n "%{$fg[red]%}''${name}''${dirty}%{$reset_color%} "
        }
        _vcs_prompt_info() {
          if jj root --quiet >/dev/null 2>&1; then
            _jj_prompt_info
          else
            git_prompt_info
          fi
        }
        # oh-my-zsh sets PROMPT when sourced later in /etc/zshrc, so we
        # can't prepend here directly. Defer the prepend to the first
        # precmd, by which point the theme has loaded.
        _prompt_warnings_install() {
          local git_prompt='$(git_prompt_info)' vcs_prompt='$(_vcs_prompt_info)'
          PROMPT="''${PROMPT//$git_prompt/$vcs_prompt}"
          PROMPT='$(_root_indicator)$(_failed_units_warning)$(_flake_age_warning)'"$PROMPT"
          precmd_functions=(''${precmd_functions:#_prompt_warnings_install})
        }
        precmd_functions+=(_prompt_warnings_install)

        bindkey $'\e[13;2u' accept-line
        bindkey $'\e[27;2;13~' accept-line
      '';

      histSize = 100000;
      histFile = "$HOME/.zsh_history";
      setOptions = [
        "HIST_IGNORE_ALL_DUPS"
      ];
    };

    environment.etc."nixpkgs-last-modified".text =
      toString flakeSelf.inputs.nixpkgs.lastModified;

    environment.sessionVariables = {
      "EDITOR" = "nvim";
      "LESS" = "FRX";
    };

    programs.tmux = {
      enable = true;
      terminal = "tmux-256color";
      extraConfig = ''
        set -s extended-keys always
        set -s extended-keys-format csi-u
        set -as terminal-features ',xterm-ghostty:extkeys,ghostty:extkeys'
      '';
    };
    programs.direnv.enable = true;
    environment.homeBinInPath = true;
    environment.localBinInPath = true;
  };
}
