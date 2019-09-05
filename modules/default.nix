{ config, pkgs, lib, ... }:

{
  imports = [
    ./basics.nix
    ./desktop.nix
    ./nginx.nix
    ./tests.nix
    ./virtualisation.nix
    ./zfs.nix
  ];

  options.me = with lib; with types; {
    propagateNix = mkEnableOption {};

    desktop = {
      enable = mkEnableOption {};
      wayland = mkEnableOption {};
    };
  };

  # Nix propagation
  config = {
    me.propagateNix = lib.mkDefault true;

    environment.etc = lib.mkIf config.me.propagateNix {
      nix-system-pkgs.source = builtins.filterSource
        (path: type:
        baseNameOf path != ".git")
        /home/svein/dev/nix/system;
      nixos.source = builtins.filterSource
        (path: type:
        baseNameOf path != ".git"
        && baseNameOf path != "secrets"
        && type != "symlink"
        && !(pkgs.lib.hasSuffix ".qcow2" path)
        && baseNameOf path != "server"
      )
      ../.;
    };
    nix.nixPath = lib.mkIf config.me.propagateNix [
      "nixpkgs=/etc/nix-system-pkgs"
    ];
  };
}
