{ config, pkgs, lib, modulesPath, ... }:

let
  keys = import ../../modules/keys.nix;
in
{
  imports = [
    "${modulesPath}/installer/cd-dvd/installation-cd-minimal.nix"
    ../../modules/default.nix # Import our base module system
  ];

  # Allow unfree packages
  nixpkgs.config.allowUnfree = true;

  # ISO-specific configuration
  isoImage = {
    isoName = lib.mkForce "nixos-custom-${config.system.nixos.label}-${pkgs.stdenv.hostPlatform.system}.iso";
    volumeID = lib.mkForce "NIXOS_ISO";

    # Enable SSH access during installation
    appendToMenuLabel = " (Custom NixOS Installer)";

    # Make the ISO a bit larger to accommodate our packages
    isoBaseName = lib.mkForce "nixos-custom";

    # Include a copy of this repository in the ISO
    contents = [
      {
        source = ../..;
        target = "/nixos-config";
      }
    ];
  };

  # Network configuration for the installer
  networking = {
    networkmanager.enable = true;
    wireless.enable = false; # Disable wpa_supplicant in favor of NetworkManager

    # Enable SSH access with password authentication for installation
    firewall = {
      enable = true;
      allowedTCPPorts = [ 22 ];
    };
  };

  # Disable WireGuard in installer
  me.wireguard.enable = false;

  # Disable SSH OTP authentication in installer
  me.sshAuth.enable = false;

  # Services for the installer environment
  services = {
    openssh = {
      enable = true;
      startWhenNeeded = false; # Start immediately, not on-demand
      settings = {
        PermitRootLogin = "prohibit-password"; # Allow root login with keys only
        PasswordAuthentication = false; # Disable password authentication
        PubkeyAuthentication = true; # Enable public key authentication
        AuthenticationMethods = "publickey"; # Only allow public key auth
      };
    };

    # Useful for debugging hardware
    fwupd.enable = true;
  };

  # Override some settings from our base modules for the installer environment
  users.include = [ ]; # Don't create our regular users

  # Configure SSH keys for root and nixos users
  users.users = {
    root = {
      openssh.authorizedKeys.keys = keys.svein.ssh;
    };
    nixos = {
      openssh.authorizedKeys.keys = keys.svein.ssh;
    };
  };

  # Additional packages useful for installation
  # Note: Many applications, including git and jujutsu are already included via cliApps.json in default.nix
  environment.systemPackages = with pkgs; [
    usbutils
    lsof

    # Text editors
    neovim
    nano

    # Partitioning tools
    parted
    gptfdisk

    # File systems
    ntfs3g
    exfat

    # Compression
    unzip
    p7zip
  ];

  # Enable experimental features for nix command and flakes
  nix.settings.experimental-features = [ "nix-command" "flakes" ];

  # Ensure we can build on the installer
  nix.settings.system-features = [ "nixos-test" "benchmark" "big-parallel" "kvm" ];

  # Console configuration
  console = {
    font = "Lat2-Terminus16";
    keyMap = "us";
  };

  # Boot configuration
  boot = {
    # Support more file systems
    supportedFilesystems = lib.mkForce [ "btrfs" "reiserfs" "vfat" "f2fs" "xfs" "ntfs" "cifs" "ext4" "zfs" ];

    # Include ZFS support
    zfs.forceImportRoot = false;

    # Kernel modules that might be useful
    kernelModules = [ ];

    # Enable firmware loading
    enableContainers = false; # Save space
  };

  # Save space by excluding some documentation
  documentation = {
    enable = true;
    nixos.enable = false;
    man.enable = true;
    info.enable = false;
    doc.enable = false;
  };

  # System configuration
  system.stateVersion = "23.11";
}
