{ ... }:

{
  imports = [
    /home/svein/dev/nix/system/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix
  ];

  boot.supportedFilesystems = [ "zfs" ];
  networking.hostId = "12345678";
}
