{ config, pkgs, lib, ... }:
let
  jobs = config.services.zrepl.settings.jobs;
  sender = lib.findFirst (job: job.name == "rpool") { } jobs;
  sink = lib.findFirst (job: job.name == "backup-sink") { } jobs;
  storage = pkgs.writeScriptBin "minecraft-storage" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings
      [ "@zfs@" "@zpool@" "@mount@" "@umount@" "@systemctl@" "@zrepl@" "@filters@" "@path@" ]
      [ "${config.boot.zfs.package}/bin/zfs" "${config.boot.zfs.package}/bin/zpool"
        "${pkgs.util-linux}/bin/mount" "${pkgs.util-linux}/bin/umount"
        "${pkgs.systemd}/bin/systemctl" "${config.services.zrepl.package}/bin/zrepl"
        (builtins.toJSON (sender.filesystems or { }))
        (lib.makeBinPath [ config.boot.zfs.package pkgs.util-linux ]) ]
      (builtins.readFile ./minecraft-storage.py)
  );
in
{
  assertions = [{
    assertion = config.services.zrepl.enable
      && (sender.type or null) == "push"
      && (sender.connect.type or null) == "local"
      && (sender.connect.listener_name or null) == "backup-sink"
      && (sender.connect.client_identity or null) == "rpool"
      && (sink.type or null) == "sink"
      && (sink.root_fs or null) == "stash/zrepl"
      && (sink.serve.type or null) == "local"
      && (sink.serve.listener_name or null) == "backup-sink";
    message = "minecraft-storage requires the rpool -> backup-sink local zrepl mapping";
  }];
  environment.systemPackages = [ storage ];
  systemd.tmpfiles.rules = [
    "d /run/minecraft-snapshot 0755 root root -"
    "d /var/lib/minecraft-storage 0700 root root -"
  ];
  # A failed or interrupted rewind must not replicate a half-restored world,
  # even if the machine reboots before the administrator retries the command.
  systemd.services.zrepl.unitConfig.ConditionPathExists =
    "!/var/lib/minecraft-storage/rollback.json";
  security.sudo.extraRules = [{
    users = [ "minecraft" ];
    runAs = "root";
    commands = [{
      command = "${storage}/bin/minecraft-storage";
      options = [ "NOPASSWD" "NOSETENV" ];
    }];
  }];
}
