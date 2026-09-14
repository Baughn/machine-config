{ pkgs, ... }:
let
  python = pkgs.python3.withPackages (p: [ p.javaproperties ]);
  hook = pkgs.writeScriptBin "minecraft-snapshot" (
    "#!${python}/bin/python3 -I\n" + builtins.readFile ./minecraft-snapshot.py
  );
  zreplHook = pkgs.writeShellScript "minecraft-zrepl-hook" ''
    exec ${pkgs.util-linux}/bin/runuser -u minecraft -- ${hook}/bin/minecraft-snapshot
  '';
in
{
  # Used by the rpool job; keep the hook beside Minecraft.
  system.build.minecraft-snapshot-hook = zreplHook;
  systemd.tmpfiles.rules = [ "d /run/minecraft-save-hook 0700 minecraft users -" ];

  systemd.services.minecraft-save-recovery = {
    description = "Recover Minecraft saving after an interrupted snapshot hook";
    serviceConfig = {
      Type = "oneshot";
      User = "minecraft";
      Group = "users";
      ExecStart = "${hook}/bin/minecraft-snapshot recover";
      TimeoutStartSec = "2m";
    };
  };
  systemd.timers.minecraft-save-recovery = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnBootSec = "1m";
      OnUnitActiveSec = "1m";
      AccuracySec = "5s";
    };
  };
}
