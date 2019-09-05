{ config, pkgs, lib, ... }:

{

  options.me = with lib; with types; {
    virtualisation.enable = mkEnableOption {};
  };

  config = lib.mkIf config.me.virtualisation.enable {
    virtualisation.lxd.enable = true;
    users.extraUsers.svein.extraGroups = [ "docker" "lxd" ];
    networking.firewall.checkReversePath = false;
    environment.systemPackages = [ pkgs.qemu ];
  };
}
