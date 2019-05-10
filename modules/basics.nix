{ config, pkgs, lib, ...}:

{
  imports = [
    ./users.nix
    ./logrotate.nix
  ];

  # F#&$*ng Spectre
  boot.kernelParams = [
    "pti=off"
    "nospectre_v1"
    "nospectre_v2"
    "l1tf=off"
    "nospec_store_bypass_disable"
    "no_stf_barrier"
    # Also, force deep sleep. This should be fine on all modern hardware.
    "mem_sleep_default=deep"
  ];

  # User setup
  users.users.root = {
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDFqQOHIaerfzhi0pQHZ/U1ES2yvql9NY46A01TjmgAl svein@tsugumi"
    ];
    inherit (import ../secrets) initialPassword;
  };
  users.defaultUserShell = "/run/current-system/sw/bin/zsh";
  users.include = [ "svein" ];
  
  # Nix propagation
  environment.etc = {
    nix-system-pkgs.source = lib.mkIf (lib.hasPrefix "/nix/store" pkgs.path) pkgs.path;
    nixos.source = builtins.filterSource
      (path: type:
      baseNameOf path != "secrets"
      && type != "symlink"
      && !(pkgs.lib.hasSuffix ".qcow2" path)
      && baseNameOf path != "server"
    )
    ../.;
  };
  nix.nixPath = lib.mkIf (lib.hasPrefix "/nix/store" pkgs.path) [ "nixpkgs=/etc/nix-system-pkgs" ];

  # Software
  documentation.dev.enable = true;
  environment.extraOutputsToInstall = [ "info" "man" "devman" ];
  programs.java.enable = true;
  programs.mosh.enable = true;
  programs.mtr.enable = true;
  programs.tmux.enable = true;
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
     gitAndTools.gitFull git-lfs sqliteInteractive
     config.boot.kernelPackages.bpftrace
     # System/monitoring/etc tools
     parted psmisc atop hdparm sdparm whois sysstat htop nload iftop
     smartmontools pciutils lsof schedtool numactl dmidecode iotop
     usbutils powertop w3m autossh
     # Shell tools
     file irssi links2 screen parallel moreutils neovim mutt finger_bsd
     autojump units progress pv mc mkpasswd ripgrep zstd
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
     # Giant lump of stuff
     zip unzip znc bsdgames shared_mime_info p7zip fortune
     gnupg unrar encfs
   ];

  environment.loginShellInit = ''
  '';

  # System setup
  ## Power
  powerManagement.enable = lib.mkDefault true;

  ## Misc.
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;
  hardware.enableKSM = true;
  hardware.enableAllFirmware = true;
  boot.loader.grub.memtest86.enable = config.boot.loader.grub.enable;
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
  services.fail2ban.enable = true;
  ### SSH
  security.pam.services.sshd.googleAuthenticator.enable = true;
  services.openssh = {
    enable = true;
    passwordAuthentication = true;
    challengeResponseAuthentication = true;
    gatewayPorts = "yes";
    forwardX11 = true;
  };
  programs.ssh.setXAuthLocation = true;

  ## Power management
  powerManagement.cpuFreqGovernor = lib.mkDefault "ondemand";

  ## Networking & Firewall basics
  networking.domain = "brage.info";
  networking.firewall.allowPing = true;
  networking.firewall.connectionTrackingModules = [ "ftp" "irc" ];
  services.avahi.enable = true;
  services.avahi.nssmdns = true;
  # Add hosts for SV.
  networking.hosts = lib.mapAttrs' (host: cfg: lib.nameValuePair cfg.publicIP [host]) (import /home/svein/sufficient/network.nix);
  
  ## Time & location ##
  i18n = {
    consoleFont = lib.mkDefault "Lat2-Terminus16";
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
  system.stateVersion = "19.03";
}
