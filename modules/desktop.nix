{
  config,
  pkgs,
  lib,
  ...
}: {
  imports = [
    ./mcupdater.nix
  ];

  services.vscode-server.enable = true;

  #boot.kernelPackages = lib.mkForce pkgs.linuxPackages_lqx;
  powerManagement.cpuFreqGovernor = lib.mkForce "schedutil";
  services.system76-scheduler.enable = true;

  ## Packages
  environment.systemPackages = with pkgs; [
    # Dev
    #((pkgs.vscode.override { isInsiders = true; }).overrideAttrs (oldAttrs: rec {
    #  src = (builtins.fetchTarball {
    #    url = "https://code.visualstudio.com/sha/download?build=insider&os=linux-x64";
    #    sha256 = "sha256:1b8lf6qqq6868kqzc35482ksfvzfxfhdpn2lisksjrji1qyiz06l";
    #  });
    #  version = "latest";
    #}))
    google-chrome
    youtube-dl
    gnome3.gnome-terminal
    steam-run
    heroic
    firefox
    xclip
    # Chat, etc.
    syncplay
    # Kanjitomo
    (pkgs.makeDesktopItem {
      name = "kanjitomo";
      exec = "${pkgs.jre}/bin/java -jar ${../third_party/KanjiTomo}/KanjiTomo.jar";
      desktopName = "KanjiTomo";
    })
    discord
    # Entertainment
    mpv
    syncplay
    # KDE utilities
    ark discover
    # Sound stuff
    helvum
    # 3D printing
    prusa-slicer
    cura
  ];

  environment.launchable.systemPackages = pkgs:
    with pkgs; [
      # Applications I rarely use
      winePackages.full
      winetricks
      blender
      pavucontrol
      ncmpcpp
      mpd
      xlockmore
      xorg.xwd
      xorg.xdpyinfo
      xorg.xev
      xorg.xkill
      glxinfo
      # Video / Photo editing
      kdenlive
      frei0r
      gimp-with-plugins #krita
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
    enableDefaultPackages = true;
    packages = with pkgs; [
      corefonts # Microsoft free fonts.
      inconsolata # Monospaced.
      ubuntu_font_family # Ubuntu fonts.
      unifont # some international languages.
      ipafont # Japanese.
      roboto # Android? Eh, it's a nice font.
    ];
  };

  programs.xwayland.enable = true;
  programs.sway = {
    enable = true;
    extraOptions = ["--unsupported-gpu"];
  };

  # Work around #224332
  environment.sessionVariables.NIXOS_OZONE_WL = "1";

  services.udev.packages = with pkgs; [gnome3.gnome-settings-daemon];

  services.xserver = {
    enable = true;
    layout = "us";
    displayManager.sddm.enable = true;
    #displayManager.lightdm.enable = true;
    #displayManager.gdm.enable = true;
    #displayManager.gdm.autoSuspend = false;
    #displayManager.gdm.autoLogin.user = "svein";
    desktopManager = {
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

  services.ratbagd.enable = true;

  hardware.pulseaudio.enable = false;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    #alsa.support32Bit = true;
    #jack.enable = true;
    pulse.enable = true;
    #media-session.enable = false;
    #wireplumber.enable = true;
  };
  hardware.bluetooth = {
    enable = true;
  };

  hardware.opengl = {
    driSupport32Bit = true;
  };
}
