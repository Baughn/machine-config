{ config, pkgs, ...}:

{
  ## Packages
  environment.systemPackages = with pkgs; [
    google-chrome steam pavucontrol youtube-dl wineFull
    gnome3.gnome_terminal compton blender gimp-with-plugins
    ncmpcpp xorg.xdpyinfo xorg.xev xorg.xkill # maim
    steam-run firefox glxinfo mpd xlockmore xorg.xwd
    idea.idea-community virtviewer
    (mpv.override {
      openalSupport = true;
    })
    (dwarf-fortress-packages.dwarf-fortress-full.override {
      enableIntro = false;
    })
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
    # windowManager.xmonad = {
    #   enable = true;
    #   enableContribAndExtras = true;
    #   extraPackages = h: with h; [
    #     MissingH
    #   ];
    # };
    xkbOptions = "ctrl:swapcaps";
    enableCtrlAltBackspace = true;
    exportConfiguration = true;

    inputClassSections = [''
      Identifier "Mouse Remap"
      MatchProduct "Mad Catz Mad Catz M.M.O.7 Mouse|M.M.O.7"
      MatchIsPointer "true"
      MatchDevicePath "/dev/input/event*"
      Option    "Buttons" "24"
      Option    "ButtonMapping" "1 2 3 4 5 0 0 8 9 10 11 12 0 0 0 16 17 7 6 0 0 0 0 0" 
      Option    "AutoReleaseButtons" "20 21 22 23 24" 
      Option    "ZAxisMapping" "4 5 6 7"
    ''];
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
  services.gnome3 = {
    chrome-gnome-shell.enable = true;
    gnome-disks.enable = true;
    gnome-terminal-server.enable = true;
    gvfs.enable = true;
  };
}
