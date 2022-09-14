{ config, pkgs, lib, ...}:

{
  imports = [ ./mcupdater.nix ];

  #boot.kernelPackages = lib.mkForce pkgs.linuxPackages_lqx;
  powerManagement.cpuFreqGovernor = lib.mkForce "performance";

  ## Packages
  environment.systemPackages = with pkgs; [
    # Dev
    vscode
    google-chrome youtube-dl
    gnome3.gnome-terminal
    steam-run firefox
    # Chat, etc.
    syncplay
    # Kanjitomo
    (pkgs.makeDesktopItem {
      name = "kanjitomo";
      exec = "${pkgs.jre}/bin/java -jar ${../third_party/KanjiTomo}/KanjiTomo.jar";
      desktopName = "KanjiTomo";
    })
    # Work around #159267
    discord
    #(pkgs.writeShellApplication {
    #  name = "discord";
    #  text = "${pkgs.discord}/bin/discord --use-gl=desktop";
    #})
    #(pkgs.makeDesktopItem {
    #  name = "discord";
    #  exec = "discord";
    #  desktopName = "Discord";
    #})
    # Entertainment
    polymc
    mpv
    syncplay
    # KDE utilities
    ark
    # Sound stuff
    helvum
    # 3D printing
    (prusa-slicer.overrideAttrs (old: {
      src = fetchFromGitHub {
        owner = "prusa3d";
        repo = "PrusaSlicer";
        sha256 = "sha256-wLe+5TFdkgQ1mlGYgp8HBzugeONSne17dsBbwblILJ4=";
        rev = "version_2.5.0";
      };
      buildInputs = old.buildInputs ++ [(
        pkgs.opencascade-occt.overrideAttrs (old: rec {
          version = "7.6.2";
          commit = "V${builtins.replaceStrings ["."] ["_"] version}";
          src = fetchurl {
            name = "occt-${commit}.tar.gz";
            url = "https://git.dev.opencascade.org/gitweb/?p=occt.git;a=snapshot;h=${commit};sf=tgz";
            sha256 = "sha256-n3KFrN/mN1SVXfuhEUAQ1fJzrCvhiclxfEIouyj9Z18=";
          };
        })
      )];
    }))
  ];

  environment.launchable.systemPackages = pkgs: with pkgs; [
    # Applications I rarely use
    winePackages.full winetricks blender pavucontrol
    ncmpcpp mpd xlockmore xorg.xwd xorg.xdpyinfo xorg.xev xorg.xkill
    glxinfo
    # Video / Photo editing
    kdenlive frei0r gimp-with-plugins #krita
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
    extraOptions = ["--my-next-gpu-wont-be-nvidia"];
  };

  services.udev.packages = with pkgs; [ gnome3.gnome-settings-daemon ];

  services.xserver = {
    enable = true;
    layout = "us";
    #displayManager.lightdm.enable = true;
    displayManager.gdm.enable = true;
    displayManager.gdm.wayland = lib.mkForce config.services.xserver.desktopManager.gnome.enable;
    desktopManager = {
#      default = "xfce";
      xfce.enable = true;
#      gnome.enable = true;
      cinnamon.enable = true;
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

  programs.xwayland.enable = true;

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
    package = pkgs.bluezFull;
  };

  hardware.opengl = {
    driSupport32Bit = true;
  };
}
