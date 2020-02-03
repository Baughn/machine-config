{ ... }:
{
  imports = [
    ./zrepl.nix
  ];
  
  boot.supportedFilesystems = [ "zfs" ];
  services.zfs.autoSnapshot.enable = true;
  services.zfs.autoSnapshot.flags = "-k -p --utc";
  boot.postBootCommands = ''
    for hd in /sys/block/sd*; do
      cd $hd; echo noop > queue/scheduler
    done
    echo 300 > /sys/module/zfs/parameters/zfs_txg_timeout

    for i in /dev/sd? ; do
      if smartctl -l scterc,70,70 $i > /dev/null ; then
          echo -n $i " is good "
      else
          echo 180 > /sys/block/''${i/\/dev\/}/device/timeout
          echo -n $i " is  bad "
      fi;
      smartctl -i $i | egrep "(Device Model|Product:)"
      blockdev --setra 1024 $i
    done
  '';
  services.zfs.autoScrub.enable = true;
}
