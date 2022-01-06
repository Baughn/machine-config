{ config, pkgs, lib, ...}:

{
  ## Packages
  environment.systemPackages = with pkgs; [
    google-chrome youtube-dl
    gnome3.gnome_terminal
    steam-run firefox
    idea.idea-community
    # Chat, etc.
    discord syncplay
    # Entertainment
    mpv
    syncplay
    # Needed for gnome in general
    gnomeExtensions.appindicator
    gnome3.adwaita-icon-theme
    # Needed for gnome to have a mouse cursor?!
    kdenlive
  ];

  environment.launchable.systemPackages = pkgs: with pkgs; [
    # Applications I rarely use
    wineFull winetricks blender pavucontrol
    ncmpcpp mpd xlockmore xorg.xwd xorg.xdpyinfo xorg.xev xorg.xkill
    glxinfo virtviewer
    # Video / Photo editing
    kdenlive frei0r gimp-with-plugins
    # One day I'll get back to this
    dwarf-fortress-packages.dwarf-fortress-full
    # Emacs
    #((emacsPackagesNgGen pkgs.emacs).emacsWithPackages (p: with p.melpaStablePackages; [
    #    solarized-theme indent-guide
    #    nyan-mode smex ein js2-mode js3-mode
    #    multiple-cursors flyspell-lazy yasnippet buffer-move counsel
    #    p.elpaPackages.undo-tree magit nix-mode gradle-mode lua-mode
    #    groovy-mode editorconfig rust-mode pabbrev expand-region
    #  ]))
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

  services.udev.packages = with pkgs; [ gnome3.gnome-settings-daemon ];

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
