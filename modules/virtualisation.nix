{ config, pkgs, ... }:

{
  virtualisation.docker.enable = true;
  virtualisation.libvirtd.enable = true;
  users.extraUsers.svein.extraGroups = [ "docker" "libvirtd" ];
  networking.firewall.checkReversePath = false;
  environment.systemPackages = [ pkgs.virtmanager ];
}
