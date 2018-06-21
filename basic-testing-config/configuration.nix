# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).

{ config, pkgs, ... }:

let
  userLib = pkgs.callPackage ../modules/users.nix {};
in

{
  imports = [
    ./hardware-configuration.nix
  ];

  ## Boot
  boot.loader.systemd-boot.enable = true;
  boot.loader.efi.canTouchEfiVariables = true;

  ## Networking
  networking.hostName = "saya";
  networking.hostId = "7a4f1297";

  users = userLib.include [ "svein" ];
  system.stateVersion = "18.03";
}
