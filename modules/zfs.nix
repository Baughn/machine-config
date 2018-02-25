{ ... }:
{
  boot.supportedFilesystems = [ "zfs" ];
  services.zfs.autoSnapshot.enable = true;
  boot.postBootCommands = ''
    for hd in /sys/block/sd*; do
      cd $hd; echo noop > queue/scheduler
    done
  '';
}
