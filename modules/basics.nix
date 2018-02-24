{ config, pkgs, lib, ...}:

let
  userLib = pkgs.callPackage ../modules/users.nix {};
in

{
  # Software
  programs.java.enable = true;
  programs.mosh.enable = true;
  programs.mtr.enable = true;
  programs.wireshark.enable = true;
  programs.zsh.enable = true;
  programs.zsh.enableAutosuggestions = true;
  programs.zsh.syntaxHighlighting.enable = true;
  programs.nano.nanorc = ''
    set nowrap
  '';

  ## System environment
  environment.systemPackages = with pkgs; [
     # Debug/dev tools
     tcpdump nmap gdb gradle python3Packages.virtualenv
     telnet man-pages posix_man_pages mono rust.cargo rust.rustc gcc stack
     pythonFull python3Full freeipmi binutils
     gitAndTools.gitFull gitAndTools.git-annex
     # System/monitoring/etc tools
     parted psmisc atop hdparm sdparm whois sysstat htop nload iftop
     smartmontools pciutils lsof schedtool nixops numactl dmidecode iotop
     # Shell tools
     file irssi links2 screen parallel moreutils vim mutt finger_bsd
     autojump units progress pv
     # File transfer
     rsync wget rtorrent unison znapzend sshfsFuse borgbackup
     # Nix tools
     nox nix-repl nix-prefetch-git
     # Video manipulation
     mkvtoolnix-cli ffmpeg
     (libav_all.override {
       x264Support = true;
     }).libav_12
     # Image-manipulation tools
     fgallery pngcrush imagemagickBig povray blender
     # Monitoring, eventually to be a module.
     prometheus prometheus-node-exporter prometheus-alertmanager
     prometheus-nginx-exporter
     # Emacs
     (emacsWithPackages (p: with p; [
          nyan-mode smex ein js2-mode js3-mode
          multiple-cursors flyspell-lazy yasnippet buffer-move helm
          flycheck magit nix-mode gradle-mode lua-mode
          groovy-mode editorconfig rust-mode
          color-theme color-theme-solarized]))
     # Giant lump of stuff
     zip unzip znc bsdgames shared_mime_info p7zip fortune
     gnupg unrar
   ];

  # User setup
  users = (userLib.include [ "svein" ]) // {
    defaultUserShell = "/run/current-system/sw/bin/zsh";
  };

  # System setup
  ## Misc.
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;
  hardware.enableKSM = true;
  zramSwap.enable = true;
  boot.cleanTmpDir = true;
  boot.kernel.sysctl = {
    "fs.inotify.max_user_watches" = 1048576;
    "net.ipv4.tcp_congestion_control" = "bbr";
  };

  ## Nix setup
  nix.buildCores = 8;
  nix.daemonIONiceLevel = 19;
  nix.daemonNiceLevel = 19;
  nix.extraOptions = "auto-optimise-store = true";
  nix.gc.automatic = true;
  nix.gc.dates = "Thu 03:15";
  nix.gc.options = "--delete-older-than 14d";
  nix.useSandbox = "relaxed";
  nixpkgs.config.allowUnfree = true;

  ## Security & Login
  security.sudo.wheelNeedsPassword = false;
  security.apparmor.enable = true;
  security.pam.enableGoogleAuth = true;
#  services.fail2ban.enable = true;
  ### SSH
  services.openssh = {
    enable = true;
    passwordAuthentication = true;
    challengeResponseAuthentication = true;
    gatewayPorts = "yes";
    forwardX11 = true;
  };
  programs.ssh.setXAuthLocation = true;

  ## ZFS setup
  boot.supportedFilesystems = [ "zfs" ];
  services.zfs.autoSnapshot.enable = true;
  boot.postBootCommands = ''
    for hd in /sys/block/sd*; do
      cd $hd; echo noop > queue/scheduler
    done
  '';

  ## Power management
  powerManagement.cpuFreqGovernor = "ondemand";

  ## Networking & Firewall basics
  networking.domain = "brage.info";
  networking.firewall.allowPing = true;
  networking.firewall.connectionTrackingModules = [ "ftp" "irc" ];
  services.avahi.enable = true;
  services.avahi.nssmdns = true;
  
  ## Time & location ##
  i18n = {
    consoleFont = "Lat2-Terminus16";
    consoleKeyMap = "us";
    defaultLocale = "en_US.UTF-8";
  };
  time.timeZone = "Europe/Dublin";

  ## Fonts
  fonts = {
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

  ## Common services
  services.locate.enable = true;
  services.cron = {
    enable = true;
    mailto = "svein";
  };
  # Enable postfix, but local only by default - no ports open.
  services.postfix.enable = true;

  # The NixOS release to be compatible with for stateful data such as databases.
  system.stateVersion = "18.03";
}
