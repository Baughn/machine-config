{ config, pkgs, ... }:

{
  virtualisation.docker.enable = true;
  virtualisation.libvirtd.enable = true;
  users.extraUsers.svein.extraGroups = [ "libvirtd" ];
  networking.firewall.checkReversePath = false;
}
