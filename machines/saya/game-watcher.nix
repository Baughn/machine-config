{ pkgs, ... }:

let
  game-watcher = pkgs.mkCranePackage {
    pname = "game-watcher";
    version = "0.1.0";
    src = ../../tools/game-watcher;
  };

  configFile = (pkgs.formats.toml { }).generate "game-watcher.toml" {
    poll_interval_ms = 1000;
    gpu_poll_interval_ms = 2000;

    games = [
      {
        name = "stationeers";
        app_id = 544550;
        firewall = [
          { proto = "udp"; port = 27015; interface = "wg0"; ipv6 = true; }
          { proto = "udp"; port = 27016; interface = "wg0"; ipv6 = true; }
        ];
      }
      {
        name = "factorio";
        app_id = 427520;
        firewall = [
          { proto = "udp"; port = 34197; ipv6 = true; }
        ];
      }
      {
        # Captain of Industry — no firewall rules, but listed so the GPU guard
        # can detect it as an active game.
        name = "captain-of-industry";
        app_id = 1594320;
        firewall = [ ];
      }
    ];

    gpu_guards = [
      {
        name = "stationeers-coi-comfyui";
        requires_any_of = [ "stationeers" "captain-of-industry" ];
        service = "comfyui.service";
        gpu_util_threshold_pct = 5;
        settle_seconds = 8;
        action = "restart";
        escalate = {
          when_triggers_exceed = 2;
          within_seconds = 600;
          action = "stop";
          applies_if_any_of = [ "stationeers" ];
        };
      }
    ];
  };
in
{
  systemd.services.game-watcher = {
    description = "Per-game firewall and service manager";
    after = [ "network-online.target" "nixos-fw.service" ];
    wants = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    # iptables/ip6tables for firewall manipulation; systemd for systemctl.
    path = [ pkgs.iptables pkgs.systemd ];
    serviceConfig = {
      Type = "simple";
      ExecStart = "${game-watcher}/bin/game-watcher --config ${configFile}";
      Restart = "on-failure";
      RestartSec = "5s";
      Environment = [
        # nvml-wrapper dlopens libnvidia-ml.so.1 at runtime.
        "LD_LIBRARY_PATH=/run/opengl-driver/lib"
        "RUST_LOG=info,game_watcher=debug"
      ];

      # Runs as root: needs CAP_NET_ADMIN for firewall and to manage other units
      # via systemctl. CAP_SYS_PTRACE lets us read other users' /proc/<pid>/cmdline.
      User = "root";
      AmbientCapabilities = [ "CAP_NET_ADMIN" "CAP_SYS_PTRACE" ];
      CapabilityBoundingSet = [ "CAP_NET_ADMIN" "CAP_SYS_PTRACE" ];

      # Hardening
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      ProtectKernelTunables = true;
      ProtectControlGroups = true;
      RestrictSUIDSGID = true;
      RestrictNamespaces = true;
      LockPersonality = true;
      MemoryDenyWriteExecute = true;
      NoNewPrivileges = true;
    };
  };
}
