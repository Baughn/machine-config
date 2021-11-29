{ config, pkgs, lib, ...}:

lib.mkIf config.me.desktop.enable {

  ## Packages
  environment.systemPackages = with pkgs; [
    google-chrome pavucontrol youtube-dl wineFull winetricks
    gnome3.gnome_terminal compton blender
    ncmpcpp xorg.xdpyinfo xorg.xev xorg.xkill # maim
    steam-run firefox glxinfo mpd xlockmore xorg.xwd
    idea.idea-community virtviewer
    # Video / Photo editing
    kdenlive frei0r gimp-with-plugins
    # Chat, etc.
    discord syncplay
    # Entertainment
    mpv
    syncplay
    (dwarf-fortress-packages.dwarf-fortress-full.override {
      enableIntro = false;
    })
    # Emacs
    ((emacsPackagesNgGen pkgs.emacs).emacsWithPackages (p: with p.melpaStablePackages; [
        solarized-theme indent-guide
        nyan-mode smex ein js2-mode js3-mode
        multiple-cursors flyspell-lazy yasnippet buffer-move counsel
        p.elpaPackages.undo-tree magit nix-mode gradle-mode lua-mode
        groovy-mode editorconfig rust-mode pabbrev expand-region
      ]))
  ];

  programs.steam.enable = true;

  ## Fonts
  fonts = {
    enableDefaultFonts = true;
    enableGhostscriptFonts = true;
    fonts = with pkgs; [
      corefonts  # Microsoft free fonts.
      inconsolata  # Monospaced.
      ubuntu_font_family  # Ubuntu fonts.
      unifont # some international languages.
      ipafont # Japanese.
      roboto # Android? Eh, it's a nice font.
    ];
  };

  programs.sway = {
    enable = true;
  };

  services.xserver = {
    enable = true;
    layout = "us";
    #displayManager.lightdm.enable = true;
    displayManager.gdm.enable = true;
    displayManager.gdm.nvidiaWayland = true;
    displayManager.gdm.wayland = true;
    desktopManager = {
#      default = "xfce";
#      xfce.enable = true;
      gnome.enable = true;
    #  cinnamon.enable = true;
    #  plasma5.enable = true;
    };
    # windowManager.xmonad = {
    #   enable = true;
    #   enableContribAndExtras = true;
    #   extraPackages = h: with h; [
    #     MissingH
    #   ];
    # };
    #xkbOptions = "ctrl:swapcaps";
    enableCtrlAltBackspace = true;
    exportConfiguration = true;
  };

  programs.xwayland.enable = true;
 
  hardware.pulseaudio.enable = false;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    jack.enable = true;
    pulse.enable = true;
  };
  hardware.bluetooth = {
    enable = true;
    package = pkgs.bluezFull;
  };

  hardware.opengl = {
    driSupport32Bit = true;
  };
}
