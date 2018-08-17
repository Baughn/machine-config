{ config, pkgs, ... }:

{
  # virtualisation.docker.enable = true;
  # virtualisation.libvirtd.enable = true;
  # virtualisation.lxc.enable = true;
  virtualisation.lxd.enable = true;
  users.extraUsers.svein.extraGroups = [ "docker" "libvirtd" "lxd" ];
  networking.firewall.checkReversePath = false;
  environment.systemPackages = [ pkgs.virtmanager pkgs.qemu ];
}
