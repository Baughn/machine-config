{ config, pkgs, ... }:

{
  imports = [
    ./basics.nix
    ./zfs.nix
  ];
}
