{ config
, pkgs
, lib
, ...
}:
let
  storage = pkgs.writeScriptBin "minecraft-storage" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings
      [ "@zfs@" "@zpool@" "@mount@" "@umount@" "@path@" ]
      [ "${config.boot.zfs.package}/bin/zfs" "${config.boot.zfs.package}/bin/zpool"
        "${pkgs.util-linux}/bin/mount" "${pkgs.util-linux}/bin/umount"
        (lib.makeBinPath [ config.boot.zfs.package pkgs.util-linux ]) ]
      (builtins.readFile ./minecraft-storage.py)
  );
in
{
  environment.systemPackages = [ storage ];
  systemd.tmpfiles.rules = [ "d /run/minecraft-snapshot 0755 root root -" ];
  security.sudo.extraRules = [{
    users = [ "minecraft" ];
    runAs = "root";
    commands = [{
      command = "${storage}/bin/minecraft-storage";
      options = [ "NOPASSWD" "NOSETENV" ];
    }];
  }];

  me.punch.groups.minecraft = {
    label = "Minecraft";
    roleIds = [ "480078714709737473" ];
    ports.tcp = [ 25565 25566 25575 ];
    ports.udp = [ 24454 ]; # Simple voice chat
  };
  services.prometheus.scrapeConfigs = [
    {
      job_name = "erisia";
      static_configs = [
        {
          targets = [ "localhost:1224" ];
        }
      ];
    }
  ];
}
