{ config, pkgs, lib, ... }:

{
  imports = [
    ./basics.nix
    ./launchable.nix
    ./nginx.nix
    ./virtualisation.nix
    ./zfs.nix
  ];
}
