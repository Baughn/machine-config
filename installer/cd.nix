{ pkgs, ... }:

{
  imports = [
    /home/svein/dev/nix/system/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix
  ];

  boot.supportedFilesystems = [ "zfs" ];
  networking.hostId = "12345678";

  # For the Dell.
  i18n.consoleFont = "${pkgs.terminus_font}/share/consolefonts/ter-u28n.psf.gz";
  boot.zfs.enableUnstable = true;
  boot.zfs.requestEncryptionCredentials = true;
}
