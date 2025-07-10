# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
{ config
, pkgs
, ...
}: {
  imports = [
    ../modules
    ../modules/amdgpu.nix
    ./hardware-configuration.nix
  ];

  # Use the gummiboot efi boot loader.
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;
  systemd.enableEmergencyMode = false; # Start up no matter what, if at all possible.
  hardware.cpu.amd.updateMicrocode = true;

  users.include = [ ];

  services.plex.enable = true;
  services.plex.openFirewall = true;

  ## Networking ##
  networking.hostName = "tromso";
  networking.hostId = "5c118177";
  networking.interfaces.internal.useDHCP = true;
  services.udev.extraRules = ''
    ACTION=="add", SUBSYSTEM=="net", ATTR{address}=="04:92:26:d8:4a:e3", NAME="internal"
  '';

  services.ddclient = {
    enable = true;
    verbose = true;
    username = "Vaughn";
    passwordFile = config.age.secrets.dyndns.path;
    server = "members.dyndns.org";
    extraConfig = ''
      custom=yes, tromso.brage.info
    '';
  };
}
