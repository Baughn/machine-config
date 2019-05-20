{ config, pkgs, lib, ... }:

{
  imports = [
    ./basics.nix
    ./zfs.nix
    ./tests.nix
    ./desktop.nix
  ];

  options.me = with lib; with types; {
    propagateNix = mkEnableOption {
      default = true;
    };

    desktop = {
      enable = mkEnableOption {};
      wayland = mkEnableOption {};
    };
  };

  # Nix propagation
  config = {
    environment.etc = lib.mkIf config.me.propagateNix {
      nix-system-pkgs.source = pkgs.path;
      nixos.source = builtins.filterSource
        (path: type:
        baseNameOf path != "secrets"
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
