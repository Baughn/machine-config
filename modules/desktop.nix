{ config, pkgs, lib, ...}:

lib.mkIf config.me.desktop.enable {

  ## Packages
  environment.systemPackages = with pkgs; [
    google-chrome steam pavucontrol youtube-dl wineFull
    gnome3.gnome_terminal compton blender
    ncmpcpp xorg.xdpyinfo xorg.xev xorg.xkill # maim
    steam-run firefox glxinfo mpd xlockmore xorg.xwd
    idea.idea-community virtviewer
    # Video / Photo editing
    kdenlive frei0r gimp-with-plugins
    # Entertainment
    (mpv.override {
      openalSupport = true;
    })
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
    # Wayland
    kwin
  ] ++ (lib.optional config.me.desktop.wayland kwin);

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

  programs.sway = lib.mkIf config.me.desktop.wayland {
    enable = true;
  };

  services.xserver = lib.mkIf (!config.me.desktop.wayland) {
    enable = true;
    displayManager.sddm = {
      enable = true;
      enableHidpi = true;
    };
    desktopManager = {
#      default = "xfce";
#      xfce.enable = true;
#      gnome3.enable = true;
      plasma5.enable = true;
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

  # Comfort
  services.redshift = {
    enable = true;
    latitude = "53.319";
    longitude = "-6.295";
  };
}
