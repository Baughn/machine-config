{ config, pkgs, ... }:

{
  virtualisation.docker.enable = true;
  users.extraUsers.svein.extraGroups = [ "docker" ];
  networking.firewall.checkReversePath = false;
}
