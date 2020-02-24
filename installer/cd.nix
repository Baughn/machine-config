{ pkgs, ... }:

{
  imports = [
    /home/svein/dev/nix/system/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix
  ];

  boot.supportedFilesystems = [ "zfs" "f2fs" ];
  networking.hostId = "12345678";

  # For the Dell.
  boot.loader.systemd-boot.consoleMode = "0";
  console.font = "${pkgs.terminus_font}/share/consolefonts/ter-u32n.psf.gz";
  boot.zfs.requestEncryptionCredentials = true;

  environment.etc.nixos-git.source = builtins.filterSource
        (path: type:
        baseNameOf path != ".git"
        && baseNameOf path != "secrets"
        && type != "symlink"
        && !(pkgs.lib.hasSuffix ".qcow2" path)
        && baseNameOf path != "server")
        ../.;
}
