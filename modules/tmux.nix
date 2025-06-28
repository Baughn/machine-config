{ pkgs, ... }:

{
  programs.tmux = {
    enable = true;
    terminal = "tmux-256color";
    keyMode = "vi";
    escapeTime = 250;
    historyLimit = 100000;
    clock24 = true;
    resizeAmount = 5;

    plugins = with pkgs.tmuxPlugins; [
      sensible
      yank
      vim-tmux-navigator
      catppuccin
    ];

    extraConfig = ''
            # Enable mouse support
            set -g mouse on
      
            # Better pane splitting
            bind | split-window -h
            bind - split-window -v
            unbind '"'
            unbind %
      
            # Activity monitoring
            setw -g monitor-activity on
            set -g visual-activity on
      
            # Automatic window renumbering
            set -g renumber-windows on
      
            # Status line configuration
            set -g status-position bottom
            set -g status-interval 5
      
            # Don't rename windows automatically
            set-option -g allow-rename off
      
            # Help display - bind to prefix+h for manual access
            bind h display-popup -w 50% -h 20 -E 'echo "
      ╭─ Tmux Help (vim keybindings enabled)─╮
      │ Navigation:                          │
      │   Ctrl+h/j/k/l  - Move between panes │
      │   | (pipe)      - Split horizontal   │
      │   - (dash)      - Split vertical     │
      │                                      │
      │ Copy Mode (prefix + [):              │
      │   h/j/k/l       - Navigate           │
      │   /             - Search forward     │
      │   ?             - Search backward    │
      │   v             - Start selection    │
      │   y             - Copy (yank)        │
      │                                      │
      │ Other:                               │
      │   prefix + :    - Command mode       │
      │   Mouse support - Enabled            │
      │   Panes start   - Index 0            │
      ╰─ Press any key to dismiss ───────────╯"; read -n 1'
      
            # Show help on new window creation
            bind n new-window \; display-popup -w 50% -h 20 -E 'echo "
      ╭─ Tmux Help (vim keybindings enabled)─╮
      │ Navigation:                          │
      │   Ctrl+h/j/k/l  - Move between panes │
      │   | (pipe)      - Split horizontal   │
      │   - (dash)      - Split vertical     │
      │                                      │
      │ Copy Mode (prefix + [):              │
      │   h/j/k/l       - Navigate           │
      │   /             - Search forward     │
      │   ?             - Search backward    │
      │   v             - Start selection    │
      │   y             - Copy (yank)        │
      │                                      │
      │ Other:                               │
      │   prefix + :    - Command mode       │
      │   Mouse support - Enabled            │
      │   Panes start   - Index 0            │
      ╰─ Press any key to dismiss ───────────╯"; read -n 1'
      
            # Show help on tmux startup
            set-hook -g session-created 'display-popup -w 50% -h 20 -E "echo \"
      ╭─ Tmux Help (vim keybindings enabled)─╮
      │ Navigation:                          │
      │   Ctrl+h/j/k/l  - Move between panes │
      │   | (pipe)      - Split horizontal   │
      │   - (dash)      - Split vertical     │
      │                                      │
      │ Copy Mode (prefix + [):              │
      │   h/j/k/l       - Navigate           │
      │   /             - Search forward     │
      │   ?             - Search backward    │
      │   v             - Start selection    │
      │   y             - Copy (yank)        │
      │                                      │
      │ Other:                               │
      │   prefix + :    - Command mode       │
      │   Mouse support - Enabled            │
      │   Panes start   - Index 0            │
      ╰─ Press any key to dismiss ───────────╯\"; read -n 1"'
    '';
  };
}
