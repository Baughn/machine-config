# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
{
  config,
  pkgs,
  ...
}: {
  imports = [
    ../modules
    ./hardware-configuration.nix
    ../modules/nvidia.nix
    ../modules/desktop.nix
    ../modules/bcachefs.nix
    #../modules/nix-serve.nix
    #./sdbot.nix
  ];

  me = {
    virtualisation.enable = true;
  };

  services.flatpak.enable = true;

  ## Boot & hardware
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  boot.kernelParams = [
    "boot.shell_on_fail"
  ];
  systemd.enableEmergencyMode = true;

  # Development
  nix.extraOptions = ''
    gc-keep-outputs = true
    gc-keep-derivations = true
  '';

  ## Networking
  networking.hostName = "saya";
  systemd.network = {
    enable = true;
    links."00-internal" = {
      linkConfig.Name = "internal";
      linkConfig.WakeOnLan = "magic";
      matchConfig.MACAddress = "f0:2f:74:8c:54:2d";
    };
    networks."20-internal" = {
      matchConfig.Name = "internal";
      DHCP = "ipv4";
    };
  };

  networking.firewall = {
    allowedTCPPorts = [
      6987 # rtorrent
    ];
    allowedUDPPorts = [
      6987 # rtorrent
      34197 # factorio
      10401 # Wireguard
    ];
  };

  users.include = [];
}
