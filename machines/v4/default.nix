{ config, lib, pkgs, ... }:

let
  sshKeys = import ../../lib/ssh-keys.nix;
in

{
  imports = [
    ../../modules
    ./hardware-configuration.nix
    ./v4proxy.nix
  ];

  # Network — single dual-stack WAN interface via systemd-networkd.
  networking.hostName = "v4";
  networking.domain = "brage.info";
  networking.useNetworkd = true;

  systemd.network = {
    enable = true;
    networks."10-wan" = {
      matchConfig.Type = "ether";
      networkConfig.Address = [ "51.75.169.212/24" "2001:41d0:801:1000::22d7/64" ];
      networkConfig.Gateway = [ "51.75.169.1" "2001:41d0:801:1000::1" ];
    };
  };

  zramSwap.enable = true;

  time.timeZone = "Europe/Dublin";
  i18n.defaultLocale = "en_US.UTF-8";

  # Opt-in shared modules (default off).
  me.security.enable = true;
  me.magicReboot.enable = true;
  me.sshAuth.enable = true;

  # SSH-only landing user for ProxyJump into the IPv6 LAN.
  users.users.minecraft = {
    isNormalUser = true;
    uid = 1018;
    createHome = false;
    openssh.authorizedKeys.keys = sshKeys.minecraft;
  };

  system.stateVersion = "23.11";
}
