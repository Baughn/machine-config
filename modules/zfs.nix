{ ... }:
{
  imports = [
    ./zrepl.nix
  ];
  
  boot.supportedFilesystems = [ "zfs" ];
  services.zfs.autoSnapshot.enable = true;
  boot.postBootCommands = ''
    for hd in /sys/block/sd*; do
      cd $hd; echo noop > queue/scheduler
    done
    echo 300 > /sys/module/zfs/parameters/zfs_txg_timeout
  '';
  services.zfs.autoScrub.enable = true;
}
