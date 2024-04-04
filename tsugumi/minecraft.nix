{
  config,
  pkgs,
  lib,
  ...
}: {
  security.sudo-rs.extraRules = [{
    users = [ "minecraft" ];
    commands = [
      { command = "/run/current-system/sw/bin/zpool status"; options = ["NOPASSWD"]; }
      { command = "/run/current-system/sw/bin/zpool list"; options = ["NOPASSWD"]; }
      { command = "/run/current-system/sw/bin/zfs list"; options = ["NOPASSWD"]; }
      { command = "/run/current-system/sw/bin/zfs snapshot rpool/minecraft"; options = ["NOPASSWD"]; }
      { command = "/run/current-system/sw/bin/zfs rollback rpool/minecraft"; options = ["NOPASSWD"]; }
      { command = "/run/current-system/sw/bin/mount -t zfs --target /home/minecraft/snapshot --source rpool/minecraft"; options = ["NOPASSWD"]; }
      { command = "/run/current-system/sw/bin/umount /home/minecraft/snapshot"; options = ["NOPASSWD"]; }
    ];
  }];
  security.sudo.extraConfig = ''
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zpool status*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zpool list*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs list*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs snapshot rpool/minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/zfs rollback rpool/minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/mount -t zfs --target /home/minecraft/snapshot --source rpool/minecraft*
    minecraft ALL= NOPASSWD: /run/current-system/sw/bin/umount /home/minecraft/snapshot
  '';
  networking.firewall.allowedTCPPorts = [ 
    25565
    25566
  ];
  networking.firewall.allowedUDPPorts = [
    24454  # Simple voice chat
    51820  # Wireguard
  ];
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
