{ pkgs, ...}:

{
  boot.supportedFilesystems = [ "zfs" "f2fs" ];
  networking.hostId = "deafbeef";
  boot.zfs.requestEncryptionCredentials = true;

  networking.networkmanager.enable = true;
  networking.wireless.enable = false;

  # Turn on flakes.
  nix.package = pkgs.nixUnstable;
  nix.extraOptions = ''
    experimental-features = nix-command flakes
  '';


  # Use a high-res font.
  boot.loader.systemd-boot.consoleMode = "0";
  console.font = "${pkgs.terminus_font}/share/consolefonts/ter-u32n.psf.gz";

  environment.etc.nixos-git.source = builtins.filterSource
        (path: type:
        baseNameOf path != ".git"
        && baseNameOf path != "secrets"
        && type != "symlink"
        && !(pkgs.lib.hasSuffix ".qcow2" path)
        && baseNameOf path != "server")
        ../.;
}
