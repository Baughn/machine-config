# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
{ config
, pkgs
, ...
}: {
  imports = [
    ../modules
    ./hardware-configuration.nix
    ../modules/nvidia.nix
    ../modules/desktop.nix
    ../modules/zfs.nix
    #../modules/nix-serve.nix
    #./sdbot.nix
  ];

  me = {
    virtualisation.enable = true;
  };


  # Build on tsugumi as well.
  nix.buildMachines = [{
    hostName = "tsugumi";
    system = "x86_64-linux";
    protocol = "ssh";
    maxJobs = 4;
    supportedFeatures = [ "kvm" "nixos-test" "big-parallel" ];
  }];
  #nix.distributedBuilds = true;
  nix.settings.cores = 8;

  services.flatpak.enable = true;

  ## Boot & hardware
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  environment.sessionVariables = {
    # Because X3D.
    WINE_CPU_TOPOLOGY = "16:0,1,2,3,4,5,6,7,16,17,18,19,20,21,22,23";
  };

  boot.kernelParams = [
    "boot.shell_on_fail"
  ];
  systemd.enableEmergencyMode = true;

  # Work around https://unix.stackexchange.com/questions/743820/what-could-cause-a-missing-mouse-scroll-event-just-after-reversing-scroll-direct
  environment.etc."libinput/local-overrides.quirks".text = ''
    [Logitech G903 LS]
    MatchName=Logitech G903 LS
    AttrEventCode=-REL_WHEEL_HI_RES;
  '';

  #services.xserver.libinput.mouse.additionalOptions = ''
  #  Option "HighResolutionWheelScrolling" "off"
  #'';

  # Run backup script on a timer, every 30 minutes.
  services.restic.backups.home = rec {
    user = "svein";
    passwordFile = "/home/svein/nixos/secrets/restic.pw";
    repository = "sftp:svein@tsugumi:short-term/backups/saya";
    backupPrepareCommand = "${pkgs.restic}/bin/restic -r ${repository} unlock";
    paths = [ "/home/svein" ];
    exclude = [
      "/home/*/.cache/*"
      "!/home/*/.cache/huggingface"
    ];
    extraBackupArgs = [ "--exclude-caches" "--compression=max" ];
    timerConfig = {
      OnCalendar = "*:0/30";
    };
    pruneOpts = [
      "--keep-hourly 36"
      "--keep-daily 7"
      "--keep-weekly 4"
      "--keep-monthly 3"
    ];
  };

  # Development
  nix.extraOptions = ''
    gc-keep-outputs = true
    gc-keep-derivations = true
  '';

  environment.systemPackages = [ config.boot.kernelPackages.perf ];

  ## Networking
  networking.hostName = "saya";
  systemd.network.networks."10-enp12s0" = {
    matchConfig.Name = "enp12s0";
    networkConfig.DHCP = "ipv4";
    networkConfig.MulticastDNS = true;
    networkConfig.LinkLocalAddressing = false;
  };

  networking.firewall = {
    allowedTCPPorts = [
      80
      443 # HTTP(S)
      6987 # rtorrent
      3000 # Textchat-ui
      25565 # Minecraft
    ];
    allowedUDPPorts = [
      6987 # rtorrent
      34197 # factorio
      10401 # Wireguard
      5200
      5201 # Stationeers
    ];
  };

  users.include = [ ];
}
