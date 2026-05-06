# Aniwatch - anime file synchronization service
{ pkgs, ... }:
let
  aniwatch = pkgs.callPackage ../../tools/aniwatch { };
in
{
  systemd = {
    services.aniwatch-sync = {
      description = "Aniwatch sync - copy new anime files";
      path = [ aniwatch ];
      serviceConfig = {
        Type = "oneshot";
        User = "svein";
        Group = "users";
      };
      script = ''
        aniwatch sync
      '';
    };

    timers.aniwatch-sync = {
      description = "Run aniwatch sync every 30 minutes";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "5min";
        OnUnitActiveSec = "30min";
        Unit = "aniwatch-sync.service";
      };
    };

    services.aniwatch-clean = {
      description = "Aniwatch clean - remove old anime files";
      path = [ aniwatch ];
      serviceConfig = {
        Type = "oneshot";
        User = "svein";
        Group = "users";
      };
      script = ''
        aniwatch clean
      '';
    };

    timers.aniwatch-clean = {
      description = "Run aniwatch clean daily";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "daily";
        Persistent = true;
        Unit = "aniwatch-clean.service";
      };
    };
  };
}
