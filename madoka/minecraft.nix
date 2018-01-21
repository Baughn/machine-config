{ config, pkgs, lib, ... }:

{
  security.sudo.extraConfig = ''
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zpool status*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zpool list*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs list*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs snapshot tank/home/minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs rollback tank/home/minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/mount -t zfs --target /home/minecraft/snapshot --source tank/home/minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/umount /home/minecraft/snapshot
  '';

  # Enable Grafana for monitoring.
  services.grafana = {
    auth.anonymous.enable = true;
    enable = true;
  };
}
