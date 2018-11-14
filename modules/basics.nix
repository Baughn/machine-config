{ config, pkgs, lib, ...}:

let
  userLib = pkgs.callPackage ../modules/users.nix {};
in

{
  # Nix propagation
  environment.etc = {
    nix-system-pkgs.source = /home/svein/dev/nix-system;
    nixos.source = builtins.filterSource
      (path: type:
      baseNameOf path != "secrets"
      && type != "symlink"
      && !(pkgs.lib.hasSuffix ".qcow2" path)
      && baseNameOf path != "server"
    )
    ../.;
  };
  nix.nixPath = [ "nixpkgs=/etc/nix-system-pkgs" ];

  # Software
  programs.java.enable = true;
  programs.mosh.enable = true;
  programs.mtr.enable = true;
  programs.wireshark.enable = true;
  programs.zsh.enable = true;
  programs.zsh.autosuggestions.enable = true;
  programs.zsh.syntaxHighlighting.enable = true;
  programs.nano.nanorc = ''
    set nowrap
  '';

  ## System environment
  environment.systemPackages = with pkgs; [
     nixops
     # Debug/dev tools
     tcpdump nmap gdb gradle python3Packages.virtualenv
     telnet man-pages posix_man_pages mono heaptrack
     rustup gcc stack
     pythonFull python3Full freeipmi binutils jq
     gitAndTools.gitFull sqliteInteractive
     # System/monitoring/etc tools
     parted psmisc atop hdparm sdparm whois sysstat htop nload iftop
     smartmontools pciutils lsof schedtool numactl dmidecode iotop
     usbutils powertop w3m autossh
     # Shell tools
     file irssi links2 screen parallel moreutils vim mutt finger_bsd
     autojump units progress pv mc mkpasswd most
     (callPackage ../tools/up {})
     # File transfer
     rsync wget rtorrent unison znapzend sshfsFuse borgbackup
     # Nix tools
     nox nix-prefetch-git
     # Video manipulation
     mkvtoolnix-cli ffmpeg-full
     (libav_all.override {
       x264Support = true;
     }).libav_12
     # Image-manipulation tools
     fgallery pngcrush imagemagickBig povray blender
     # Monitoring, eventually to be a module.
     prometheus prometheus-node-exporter prometheus-alertmanager
     prometheus-nginx-exporter
     # Emacs
     ((emacsPackagesNgGen pkgs.emacs).emacsWithPackages (p: with p.melpaStablePackages; [
          solarized-theme indent-guide
          nyan-mode smex ein js2-mode js3-mode
          multiple-cursors flyspell-lazy yasnippet buffer-move counsel
          p.elpaPackages.undo-tree magit nix-mode gradle-mode lua-mode
          groovy-mode editorconfig rust-mode pabbrev expand-region
          ]))
     # Giant lump of stuff
     zip unzip znc bsdgames shared_mime_info p7zip fortune
     gnupg unrar
   ];

  environment.loginShellInit = ''
    export PAGER=${pkgs.most}/bin/most
  '';

  # User setup
  users = (userLib.include [ "svein" ]) // {
    defaultUserShell = "/run/current-system/sw/bin/zsh";
  };

  # System setup
  ## Misc.
  powerManagement.enable = false;
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;
  hardware.enableKSM = true;
  # zramSwap.enable = true;
  boot.cleanTmpDir = true;
  boot.kernel.sysctl = {
    "fs.inotify.max_user_watches" = 1048576;
    "net.ipv4.tcp_congestion_control" = "bbr";
  };

  ## Nix setup
  nix.buildCores = lib.mkDefault 0;
  nix.daemonIONiceLevel = 7;
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

  ## Power management
  powerManagement.cpuFreqGovernor = "ondemand";

  ## Networking & Firewall basics
  networking.domain = "brage.info";
  networking.firewall.allowPing = true;
  networking.firewall.connectionTrackingModules = [ "ftp" "irc" ];
  services.avahi.enable = true;
  services.avahi.nssmdns = true;
  # Add hosts for SV.
  networking.hosts = lib.mapAttrs' (host: ip: lib.nameValuePair ip [(host + ".sv")]) (import /home/svein/sufficient/machines.nix).machines;
  
  ## Time & location ##
  i18n = {
    consoleFont = "Lat2-Terminus16";
    consoleKeyMap = "us";
    defaultLocale = "en_US.UTF-8";
  };
  time.timeZone = "Europe/Dublin";

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
