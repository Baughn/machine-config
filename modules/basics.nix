{
  config,
  pkgs,
  lib,
  ...
}: {
  imports = [
    ./users.nix
    ./logrotate.nix
    ./naming.nix
  ];

  # Use whatever kernel package is compatible.
  boot.kernelPackages = lib.mkDefault config.boot.zfs.package.latestCompatibleLinuxPackages;
  boot.kernelParams = [
    # F#&$*ng Spectre
    "noibrs"
    "noibpb"
    "nopti"
    "nospectre_v1"
    "nospectre_v2"
    "l1tf=off"
    "nospec_store_bypass_disable"
    "no_stf_barrier"
    "mds=off"
    "mitigations=off"
  ];

  boot.loader.timeout = 15;

  # Performance stuff
  #nix.daemonCPUSchedPolicy = "idle";
  security.rtkit.enable = true;
  services.ananicy.enable = true;

  # User setup
  users.mutableUsers = false;
  users.users.root = {
    openssh.authorizedKeys.keys = (import ./sshKeys.nix).svein;
    passwordFile = config.age.secrets.userPassword.path;
  };
  users.defaultUserShell = pkgs.zsh;
  users.include = ["svein"];
  environment.variables.EDITOR = "nvim";

  # Software
  documentation.dev.enable = true;
  environment.extraOutputsToInstall = ["man" "devman"];
  programs.dconf.enable = true; # Needed for settings by various apps
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
    # Debug/dev tools
    tcpdump
    nmap
    gdb
    gradle
    python3Packages.virtualenv
    inetutils
    man-pages
    posix_man_pages
    mono
    heaptrack
    gcc
    rustup
    cargo-edit
    rust-analyzer
    python3Full
    freeipmi
    binutils
    jq
    gitAndTools.gitFull
    git-lfs
    git-crypt
    sqlite-interactive
    # System/monitoring/etc tools
    parted
    psmisc
    atop
    hdparm
    sdparm
    whois
    sysstat
    htop
    nload
    iftop
    smartmontools
    pciutils
    lsof
    schedtool
    numactl
    dmidecode
    iotop
    usbutils
    powertop
    w3m
    autossh
    # Shell tools
    file
    weechat
    parallel
    moreutils
    neovim
    finger_bsd
    autojump
    ripgrep
    zstd
    fd
    rlwrap
    (callPackage ../tools/up {})
    # AI support
    shell_gpt
    # File transfer
    rsync
    wget
    rtorrent
    sshfs-fuse
    # Nix tools
    nox
    nix-prefetch-git
    # Monitoring, eventually to be a module.
    prometheus
    prometheus-node-exporter
    prometheus-alertmanager
    prometheus-nginx-exporter
    # Giant lump of stuff
    shared-mime-info
    p7zip
    fortune
  ];

  environment.launchable.systemPackages = pkgs:
    with pkgs; [
      # Games
      nethack
      steamcmd
      steam-run
      # Tools
      unrar
      znc
      progress
      pv
      pixz
      mbuffer
      mc
      mkpasswd
      units
      gnupg
      encfs
      btop
      links2
      unison
      borgbackup
      imagemagickBig
      zip
      unzip
      # Image-manipulation tools
      fgallery
      pngcrush
      povray
      # Video manipulation
      mkvtoolnix-cli
      ffmpeg
    ];

  environment.loginShellInit = ''
    # Makes touchscreens work in Firefox. Ish.
    export MOZ_USE_XINPUT2=1
  '';

  # System setup
  ## Power
  powerManagement.enable = lib.mkDefault true;

  ## Misc.
  hardware.cpu.intel.updateMicrocode = true;
  hardware.cpu.amd.updateMicrocode = true;
  #hardware.enableKSM = true;
  hardware.enableAllFirmware = true;
  boot.loader.grub.memtest86.enable = config.boot.loader.grub.enable;
  services.fwupd.enable = true;
  boot.cleanTmpDir = true;
  boot.kernel.sysctl = {
    "fs.inotify.max_user_watches" = 1048576;
    "net.ipv4.tcp_congestion_control" = "bbr";
    "net.ipv4.tcp_ecn" = 1;
    "vm.swappiness" = 100;
  };
  zramSwap.enable = true;

  ## Nix setup
  nix.gc.automatic = true;
  nix.gc.dates = "Thu 03:15";
  nix.gc.options = lib.mkDefault "--delete-older-than 14d";
  nix.settings = {
    cores = lib.mkDefault 0;
    sandbox = "relaxed";
    trusted-users = ["root" "svein"];
  };
  nix.nrBuildUsers = 48;
  nixpkgs.config.allowUnfree = true;
  nix.package = pkgs.nixFlakes;
  nix.extraOptions = ''
    auto-optimise-store = true
    experimental-features = nix-command flakes
  '';

  ## Security & Login
  security.sudo.wheelNeedsPassword = false;
  security.apparmor.enable = true;
  services.fail2ban.enable = true;
  ### SSH
  security.pam.services.sshd.googleAuthenticator.enable = true;
  services.openssh = {
    enable = true;
    passwordAuthentication = true;
    kbdInteractiveAuthentication = true;
    gatewayPorts = "yes";
    forwardX11 = true;
    sftpServerExecutable = "internal-sftp";
  };
  programs.ssh.setXAuthLocation = true;

  ## Power management
  powerManagement.cpuFreqGovernor = lib.mkDefault "ondemand";

  ## Networking & Firewall basics
  networking.domain = "brage.info";
  networking.firewall.allowPing = true;
  networking.firewall.logRefusedConnections = false;
  ### Open ports for mosh.
  networking.firewall.allowedUDPPortRanges = [
    {
      from = 60000;
      to = 61000;
    }
  ];
  services.avahi = {
    enable = lib.mkDefault true;
    nssmdns = true;
    interfaces = ["internal"];
    publish.enable = true;
    publish.addresses = true;
    publish.workstation = true;
  };

  # Add hosts for SV.
  networking.hosts = lib.mapAttrs' (host: cfg: lib.nameValuePair cfg.publicIP [(host + ".sv")]) (
    import ../secrets/sv-network.nix
  );

  ## Time & location ##
  console = {
    font = "Lat2-Terminus16";
    useXkbConfig = true; # use xkbOptions in tty.
  };
  i18n.defaultLocale = "en_US.UTF-8";
  time.timeZone = "Europe/Dublin";

  # Enable postfix, but local only by default - no ports open.
  services.postfix.enable = true;
}
