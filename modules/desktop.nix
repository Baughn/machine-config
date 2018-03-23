{ config, pkgs, ...}:

{
  ## Packages
  environment.systemPackages = with pkgs; [
    google-chrome steam pavucontrol mpv youtube-dl wine
    gnome3.gnome_terminal compton blender gimp-with-plugins
    ncmpcpp xorg.xdpyinfo xorg.xev xorg.xkill # maim
    steam-run firefox glxinfo mpd xlockmore xorg.xwd
  ];

  ## Fonts
  fonts = {
    enableDefaultFonts = true;
    enableFontDir = true;
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

  services.xserver = {
    enable = true;
    desktopManager = {
#      default = "xfce";
#      xfce.enable = true;
      gnome3.enable = true;
#      plasma5.enable = true;
    };
    displayManager.gdm.enable = true;
    windowManager.xmonad = {
      enable = true;
      enableContribAndExtras = true;
      extraPackages = h: with h; [
        MissingH
      ];
    };
    xkbOptions = "ctrl:swapcaps";
    enableCtrlAltBackspace = true;
    exportConfiguration = true;
  };

  hardware.pulseaudio = {
    enable = true;
    support32Bit = true;
  };

  hardware.opengl = {
    enable = true;
    driSupport32Bit = true;
    s3tcSupport = true;
  };

  services.udisks2.enable = config.services.xserver.enable;
}
