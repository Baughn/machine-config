{ config, pkgs, lib, ... }:

{
  security.sudo.extraConfig = ''
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zpool status*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zpool list*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs list*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs snapshot minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs rollback minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/mount -t zfs --target /home/minecraft/snapshot --source minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/umount /home/minecraft/snapshot
  '';
}
