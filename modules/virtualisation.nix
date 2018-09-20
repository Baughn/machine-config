{ config, pkgs, ... }:

{
  virtualisation.docker.enable = false;
  virtualisation.lxd.enable = true;
  users.extraUsers.svein.extraGroups = [ "docker" "lxd" ];
  networking.firewall.checkReversePath = false;
  environment.systemPackages = [ pkgs.qemu ];
}
