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
    # Chat, etc.
    discord syncplay
    # Entertainment
    (mpv.override {
      openalSupport = true;
    })
    (pkgs.python37.pkgs.callPackage <nixpkgs/pkgs/applications/networking/syncplay> { })
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
    layout = "us";
    displayManager.sddm = {
      enable = true;
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
  };
 
  sound.enable = true;
  hardware.pulseaudio = {
    enable = true;
    support32Bit = true;
    package = pkgs.pulseaudioFull;
    extraModules = [ pkgs.pulseaudio-modules-bt ];
  };
  hardware.bluetooth = {
    enable = true;
    package = pkgs.bluezFull;
  };

  hardware.opengl = {
    driSupport32Bit = true;
    s3tcSupport = true;
  };
}
