{
  config,
  pkgs,
  lib,
  ...
}: {
  networking.hostId = lib.mkDefault "deafbeef";
  services.zfs.autoSnapshot.enable = lib.mkDefault true;
  services.zfs.autoSnapshot.flags = "-k -p --utc";
  boot.postBootCommands = ''
    #for hd in /sys/block/sd*; do
    #  cd $hd; echo noop > queue/scheduler
    #done
    echo 60 > /sys/module/zfs/parameters/zfs_txg_timeout
  '';
  services.zfs.autoScrub.enable = true;
  boot.kernelPackages = lib.mkForce config.boot.zfs.package.latestCompatibleLinuxPackages;
}
