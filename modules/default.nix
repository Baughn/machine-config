{ config, pkgs, ... }:

{
  imports = [
    ./basics.nix
    ./zfs.nix
    ./tests.nix
  ];
}
