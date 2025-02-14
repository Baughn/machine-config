{
  config,
  pkgs,
  lib,
  ...
}: {
  options.me = with lib;
  with types; {
    virtualisation.enable = mkEnableOption {};
  };

  config = lib.mkIf config.me.virtualisation.enable {
    virtualisation.libvirtd.enable = false;
    virtualisation.lxd.enable = false;
    virtualisation.docker = {
      enable = true;
    };
    hardware.nvidia-container-toolkit.enable = true;
    users.extraUsers.svein.extraGroups = ["docker" "lxd" "libvirtd"];
    networking.firewall.checkReversePath = false;
    environment.systemPackages = [
      pkgs.qemu
      pkgs.nixos-shell
      pkgs.lxd-lts
      pkgs.docker-compose
    ];
  };
}
