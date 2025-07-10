{pkgs, ...}: {
  imports = [
    ../modules/bcachefs.nix
  ];

  networking.networkmanager.enable = true;
  networking.wireless.enable = false;

  # Turn on flakes.
  nix.package = pkgs.nixFlakes;
  nix.extraOptions = ''
    experimental-features = nix-command flakes
  '';

  # Use a high-res font.
  boot.loader.systemd-boot.consoleMode = "0";
  console.font = "${pkgs.terminus_font}/share/consolefonts/ter-u32n.psf.gz";

  environment.etc.nixos-git.source =
    builtins.filterSource
    (path: type:
      baseNameOf path
      != ".git"
      && baseNameOf path != "secrets"
      && type != "symlink"
      && !(pkgs.lib.hasSuffix ".qcow2" path)
      && baseNameOf path != "server")
    ../.;
}
