{ config, pkgs, lib, ... }:

{
  imports = [
    ../secrets
    ./basics.nix
    ./launchable.nix
    ./nginx.nix
    ./virtualisation.nix
    ./zfs.nix
    ./resilience.nix
  ];
}
