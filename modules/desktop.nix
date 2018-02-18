{ config, pkgs, ...}:

{
  environment.systemPackages = with pkgs; [
    google-chrome steam pavucontrol mpv youtube-dl wine
    gnome3.gnome_terminal compton blender gimp-with-plugins
    maim ncmpcpp xorg.xdpyinfo xorg.xev xorg.xkill
    steam-run firefox glxinfo mpd xlockmore xorg.xwd
  ];

  services.xserver = {
    enable = true;
    desktopManager = {
      default = "xfce";
      xfce.enable = true;
      gnome3.enable = false;
      plasma5.enable = false;
    };
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
