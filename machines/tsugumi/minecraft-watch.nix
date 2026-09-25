{ config, lib, pkgs, ... }:
# Snapshot watchdog. Root-owned, so no agent can change or silence it.
# See docs/agent-channel-design.md, "Snapshot watchdog".
let
  cfg = config.me.minecraft.watch;
  roster = import ../../lib/agent-roster.nix;
  sender = lib.findFirst (job: job.name == "rpool") { } config.services.zrepl.settings.jobs;
  watch = pkgs.writeScriptBin "minecraft-watch" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings
      [ "@zfs@" "@systemctl@" "@filters@" "@autostart@" "@owner@" "@webhook@" "@settings@" ]
      [ "${config.boot.zfs.package}/bin/zfs" "${pkgs.systemd}/bin/systemctl"
        (builtins.toJSON (sender.filesystems or { }))
        (builtins.toJSON config.me.minecraft.autostart)
        (toString roster.humans.baughn.discordId)
        cfg.webhookFile
        (builtins.toJSON cfg.settings) ]
      (builtins.readFile ./minecraft-watch.py)
  );
in
{
  options.me.minecraft.watch = {
    webhookFile = lib.mkOption {
      type = lib.types.str;
      description = "File holding the Discord webhook URL, readable by root only.";
    };
    settings = lib.mkOption {
      type = lib.types.attrsOf lib.types.int;
      default = {
        snapshotAge = 45 * 60; # three snapshot intervals
        replicaAge = 2 * 3600;
        leaseAge = 10 * 60;
        bootGrace = 45 * 60; # no snapshot/replica verdicts right after boot
        loopRestarts = 3;
        loopWindow = 3600;
        confirm = 2; # consecutive failing runs before a key fires
      };
      description = "Thresholds, in seconds where they are durations.";
    };
  };

  config = {
    systemd.services.minecraft-watch = {
      description = "Minecraft snapshot watchdog";
      after = [ "zfs.target" ];
      # Manual runs (and the VM test) may follow each other quickly.
      startLimitIntervalSec = 0;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${watch}/bin/minecraft-watch";
        StateDirectory = "minecraft-watch";
        StateDirectoryMode = "0700";
        TimeoutStartSec = "2m";
        ProtectHome = "read-only";
        PrivateTmp = true;
      };
    };
    systemd.timers.minecraft-watch = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "15min";
        OnUnitActiveSec = "5min";
      };
    };
    # `minecraft-watch status` shows every check; `minecraft-watch test` posts.
    environment.systemPackages = [ watch ];
  };
}
