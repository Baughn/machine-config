{ config, pkgs, lib, ... }:

{

  options.me = with lib; with types; {
    virtualisation.enable = mkEnableOption {};
  };

  config = lib.mkIf config.me.virtualisation.enable {
    virtualisation.libvirtd.enable = true;
    virtualisation.lxd.enable = true;
    virtualisation.docker.enable = true;
    users.extraUsers.svein.extraGroups = [ "docker" "lxd" "libvirtd" ];
    networking.firewall.checkReversePath = false;
    environment.systemPackages = [ pkgs.qemu pkgs.virtmanager ];
  };
}
