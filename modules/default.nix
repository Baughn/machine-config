{ config, pkgs, lib, ... }:

{
  imports = [
    ../secrets
    ./basics.nix
    ./launchable.nix
    ./nginx.nix
    ./resilience.nix
    ./virtualisation.nix
    ./wireguard.nix
    ./zfs.nix
  ];
}
