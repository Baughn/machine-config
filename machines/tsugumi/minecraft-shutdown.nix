{ config, pkgs, ... }:
let
  uid = toString config.users.users.minecraft.uid;
in
{
  systemd.services.minecraft-shutdown = {
    description = "Give Minecraft servers a bounded graceful shutdown window";
    wantedBy = [ "multi-user.target" ];
    # Stop ordering is the reverse of start ordering. Keep tmux, Java scopes,
    # the user bus, runtime directory and network alive until the hook finishes.
    after = [ "user@${uid}.service" "user.slice" "network.target" ];
    wants = [ "user@${uid}.service" ];
    unitConfig.RequiresMountsFor = [ "/home/minecraft" ];

    # A switch must only update the unit definition, never run ExecStop.
    restartIfChanged = false;
    stopIfChanged = false;
    path = [ pkgs.bash pkgs.coreutils pkgs.tmux ];
    environment = {
      HOME = "/home/minecraft";
      XDG_RUNTIME_DIR = "/run/user/${uid}";
    };
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      User = "minecraft";
      Group = "users";
      ExecStart = "${pkgs.coreutils}/bin/true";
      # Runtime builder checkout owns server management and per-world controls.
      ExecStop = "${pkgs.python3}/bin/python3 /home/minecraft/builder/shutdown.py --grace 10";
      TimeoutStopSec = "60s";
      # Kill the entire hook cgroup immediately at the deadline, without a
      # second TimeoutStopSec wait for a stuck control process to obey SIGTERM.
      TimeoutStopFailureMode = "kill";
      KillMode = "control-group";
    };
  };
}
