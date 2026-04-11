{ config, lib, pkgs, ... }:

{
  imports = [
    ../../modules
    ../../kwin-bug/drm-atomic-log.nix
    ./hardware-configuration.nix
    ./cachy-tweaks.nix
    ./ganbot.nix
    ./game-watcher.nix
  ];

  # Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  # Network
  networking.hostName = "saya";
  networking.networkmanager.enable = true;
  networking.networkmanager.dns = "systemd-resolved";
  networking.firewall.allowPing = false;
  me.mdns.enable = true;
  me.mdns.publish = true;
  me.security.enable = true;
  me.firejail.enable = true;

  # WireGuard hub
  me.wireguard = {
    enable = true;
    address = [ "10.42.0.1/24" "fd10:42::1/64" ];
    privateKeyFile = "/etc/wireguard/private.key";
    listenPort = 51820;
    peers = [
      # Kim
      {
        publicKey = "y4IVDKFfuEoU9Xiq+nmY8wkMUAkE8wfwSpY/p7S+qEk=";
	allowedIPs = [ "10.42.0.2/32" ];
      }
      # jrddunbr
      {
        publicKey = "QPSh4TROwtw54n9Xb/VvCHN0TQpm6417p7Gl+//7VVg=";
	allowedIPs = [ "10.42.0.3/32" ];
	endpoint = "ctha.ja4.org:51820";
      }
    ];
  };

  # Locale
  time.timeZone = "Europe/Dublin";
  i18n.defaultLocale = "en_US.UTF-8";
  console = {
    font = "Lat2-Terminus16";
    keyMap = "us";
  };

  # Audio
  services.pipewire = {
    enable = true;
    pulse.enable = true;
  };

  # GPU
  services.xserver.enable = true;
  services.xserver.videoDrivers = [ "nvidia" ];
  hardware.nvidia = {
    modesetting.enable = true;
    open = true;
    nvidiaSettings = true;
    package = config.boot.kernelPackages.nvidiaPackages.beta;
  };

  # Fonts
  fonts.fontconfig = {
    subpixel.rgba = "none";
    hinting.style = "slight";
    antialias = true;
  };

  # Desktop environment
  services.displayManager.enable = true;
  services.displayManager.sddm.enable = true;
  services.displayManager.sddm.wayland.enable = true;
  services.displayManager.sddm.wayland.compositor = "kwin";
  services.desktopManager.plasma6.enable = true;
  drm-atomic-log.enable = true;

  # User
  users.users.svein = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keys = [
      "cert-authority ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGfsmAbJ1GKytVA71izC3xvIFYDQVHT2Q5CZPaIA6WqS svein@tsugumi"
    ];
  };


  # Desktop packages (not shared across machines)
  environment.systemPackages = [
    pkgs.ghostty
    pkgs.firefox
    pkgs.discord
    pkgs.nvtopPackages.nvidia
    pkgs.btop-cuda
    pkgs.zed-editor
  ];

  # Nix build parallelism (machine-specific: 16-core CPU)
  nix.settings.cores = 16;

  # Desktop programs
  programs.steam.enable = true;

  # Automatic security updates (no auto-reboot for a desktop)
  system.autoUpgrade = {
    enable = true;
    flake = "/home/svein/cachy-nix#saya";
    dates = "04:00";
    allowReboot = false;
  };

  # nixos-upgrade runs as root but the flake lives in svein's home;
  # libgit2 refuses cross-user repos unless listed in safe.directory.
  programs.git.config.safe.directory = "/home/svein/cachy-nix";

  system.stateVersion = "25.11";
}
