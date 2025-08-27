{ config, lib, pkgs, ... }:

let
  # Custom Oh-My-Zsh theme package
  sunakunCustomTheme = pkgs.stdenv.mkDerivation {
    pname = "oh-my-zsh-sunaku-custom-theme";
    version = "1.0.0";

    src = pkgs.writeTextFile {
      name = "sunaku-custom.zsh-theme";
      text = ''
        # Git prompt settings
        ZSH_THEME_GIT_PROMPT_ADDED="%{$fg[green]%}+"
        ZSH_THEME_GIT_PROMPT_MODIFIED="%{$fg[magenta]%}!"
        ZSH_THEME_GIT_PROMPT_DELETED="%{$fg[red]%}-"
        ZSH_THEME_GIT_PROMPT_RENAMED="%{$fg[blue]%}>"
        ZSH_THEME_GIT_PROMPT_UNMERGED="%{$fg[cyan]%}#"
        ZSH_THEME_GIT_PROMPT_UNTRACKED="%{$fg[yellow]%}?"

        ZSH_THEME_GIT_PROMPT_PREFIX=""
        ZSH_THEME_GIT_PROMPT_SUFFIX=" "
        ZSH_THEME_GIT_PROMPT_DIRTY=""
        ZSH_THEME_GIT_PROMPT_CLEAN=""

        local user_color='yellow'
        test $UID -eq 0 && user_color='red'

        _sunakayu_vcs_status() {
          # Skip jj operations when running as root to avoid ownership issues
          if [[ $UID -eq 0 ]]; then
            git_prompt_status
            return
          fi
          
          local ref='self.change_id().shortest(3)'
          local empty_color="$fg[green]"
          local nonempty_color="$fg[magenta]"

          jj_prompt_template_raw "if(self.empty(), \"%{$empty_color%}\", \"%{$nonempty_color%}\") ++ $ref ++ \" \"" \
          || git_prompt_status
        }

        _sunakayu_vcs_info() {
          # Skip jj operations when running as root to avoid ownership issues
          if [[ $UID -eq 0 ]]; then
            git_prompt_info
            return
          fi
          
          jj_prompt_template 'surround("", " ", self.description().first_line())' \
          || git_prompt_info
        }

        PROMPT='%(?..%{$fg_bold[red]%}exit %?
        %{$reset_color%})'\
        '%{$bold_color%}$(_sunakayu_vcs_status)%{$reset_color%}'\
        '$(_sunakayu_vcs_info)'\
        '%{$fg[cyan]%}%m%{$reset_color%} '\
        '%{$fg[$user_color]%}%~%{$reset_color%}'\
        '%(!.#.>) '

        PROMPT2='%{$fg[red]%}\ %{$reset_color%}'
      '';
    };

    dontUnpack = true;

    installPhase = ''
      mkdir -p $out/share/zsh/themes
      cp $src $out/share/zsh/themes/sunaku-custom.zsh-theme
    '';
  };

in
{
  # AIDEV-NOTE: Custom Zsh configuration with sunaku-based theme using customPkgs
  programs.zsh = {
    enable = true;

    # Enable autosuggestions from history
    autosuggestions.enable = true;

    # Oh My Zsh configuration
    ohMyZsh = {
      enable = true;
      theme = "sunaku-custom";
      customPkgs = [ sunakunCustomTheme ];
      plugins = [
        "sudo"
        "git"
        "jj"
        "ssh"
      ];
    };
  };
}
