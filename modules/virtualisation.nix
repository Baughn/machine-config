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
    virtualisation.docker.enable = true;
    users.extraUsers.svein.extraGroups = ["docker" "lxd" "libvirtd"];
    networking.firewall.checkReversePath = false;
    virtualisation.podman = {
      enable = true;
      enableNvidia = true;
      autoPrune.enable = true;
      defaultNetwork.settings = {
        dns_enabled = true;
      };
    };
    environment.systemPackages = [
      pkgs.qemu
      pkgs.nixos-shell
      pkgs.lxd
    ];
  };
}
