{
  config,
  pkgs,
  lib,
  ...
}:

{
  options = {
    me.monitoring = {
      enable = lib.mkEnableOption {
        description = "Enable monitoring services";
        default = true;
      };
      zfs = lib.mkEnableOption {
        description = "Enable ZFS monitoring";
        default = true;
      };
    };
  };

  config = lib.mkIf config.me.monitoring.enable {
    systemd.services.alertmanager-discord = {
      requires = ["network-online.target"];
      after = ["network-online.target"];
      wantedBy = ["multi-user.target"];
      script = ''
        source ${config.age.secrets.monitoringWebhook.path}
        ${pkgs.callPackage monitoring/alertmanager-discord {}}/bin/alertmanager-discord --listen.address=0.0.0.0:9095
      '';
      serviceConfig = {
        Restart = "always";
      };
    };

    systemd.services.prometheus-zpool-exporter = lib.mkIf config.me.monitoring.zfs {
      description = "Export current pool error status, for alerting";
      wantedBy = ["multi-user.target"];
      script = ''
        cd /run/prometheus-node-exporter
        while true; do
          echo -n > zpool
          for pool in $(${pkgs.zfs}/bin/zpool list -H | ${pkgs.gawk}/bin/awk '{print $1}'); do
            if ${pkgs.zfs}/bin/zpool status -x | grep -q $pool; then
              if ${pkgs.zfs}/bin/zpool status | grep -A1 $pool | grep -q ONLINE; then
                echo "zfs_pool_errors{pool=\"$pool\"} 0" >> zpool
              else
                echo "zfs_pool_errors{pool=\"$pool\"} 1" >> zpool
              fi
            else
              echo "zfs_pool_errors{pool=\"$pool\"} 0" >> zpool
            fi
          done
          mv zpool zpool.prom
          sleep 30
        done
      '';
    };

    systemd.services.prometheus-node-exporter.serviceConfig.ProtectHome = lib.mkForce false;

    services.prometheus = {
      enable = true;
      alertmanager = {
        enable = true;
        port = 9093;
        configuration = {
          route = {
            group_by = ["alertname"];
            group_wait = "30s";
            repeat_interval = "1h";
            receiver = "discord";
          };
          receivers = [
            {
              name = "discord";
              webhook_configs = [
                {
                  send_resolved = true;
                  url = "http://${config.networking.hostName}:9095";
                }
              ];
            }
          ];
        };
      };
      alertmanagers = [
        {
          scheme = "http";
          #path_prefix = "/alertmanager";
          static_configs = [
            {
              targets = ["${config.networking.hostName}:${builtins.toString config.services.prometheus.alertmanager.port}"];
            }
          ];
        }
      ];
      rules = [
        ''
          groups:
          - name: system
            rules:
            - alert: MonitoringDown
              expr: up == 0
              for: 5m
              annotations:
                summary: "Monitoring process is down or unreachable"
                description: "{{ $labels.instance }} not reachable."
          - name: zfs
            rules:
            - alert: PoolErrors
              expr: zfs_pool_errors > 0
              annotations:
                summary: ZFS pool errors
                description: "Pool errors detected on {{ $labels.pool }}"
          - name: machine
            rules:
            - alert: WaterTempHigh
              expr:  node_hwmon_sensor_label{label="Coolant temp"} * on (chip) group_right node_hwmon_temp_celsius > 43
              annotations:
                summary: "Coolant temperature is high"
                description: "Coolant temperature is {{ $value }} degrees on {{ $labels.instance }}"
            - alert: PumpSpeedLow
              expr: node_hwmon_sensor_label{label="Pump speed"} * on (chip) group_right node_hwmon_fan_rpm{sensor="fan1"} < 1600
              annotations:
                summary: "Pump speed is low"
                description: "Pump speed is {{ $value }} RPM on {{ $labels.instance }}"
            - alert: MemoryErrors
              expr: node_edac_correctable_errors_total > 0
              annotations:
                summary: "Correctable EDAC errors"
                description: "{{ $value }} correctable errors detected"
            - alert: MemoryFailure
              expr: node_edac_uncorrectable_errors_total > 0
              annotations:
                summary: "Uncorrectable EDAC errors"
                description: "{{ $value }} uncorrectable errors detected"
            - alert: TemperatureHigh
              expr: node_hwmon_temp_celsius > 92
              for: 2m
              annotations:
                summary: "CPU is literally on fire"
                description: "{{ $value }} degrees on {{ $labels.instance }}"
            - alert: CPUStealHigh
              expr: sum(rate(node_cpu_seconds_total{mode="steal"}[5m])) by(instance) > 0.2
              for: 15m
              annotations:
                summary: "CPU steal is {{ $value }} (fraction)"
                description: "See https://control.sufficientvelocity.com/grafana/d/9CWBz0bik/machine-stats"
            - alert: DiskUseHigh
              expr: node_filesystem_avail_bytes / node_filesystem_size_bytes < 0.1
              annotations:
                summary: "Filesystem space use > 90%"
                description: "S{{ $value }} (fraction) free on {{ $labels.instance }} {{ $labels.mountpoint }}"
            - alert: DiskFreeSpaceLow
              expr: node_filesystem_avail_bytes{fstype!~"vfat|fuse.*|ramfs|tmpfs"} < 10000000000
              annotations:
                summary: "Too little free space left."
                description: "{{ $value }} bytes free on {{ $labels.instance }} {{ $labels.mountpoint }}"
                details: "Working around https://github.com/prometheus/node_exporter/issues/1498"
            - alert: FilesystemScrapeErrors
              expr: node_filesystem_device_error{fstype!~"tmpfs|fuse.*|ramfs"} > 0
              annotations:
                description: "{{ $value }} filesystem scrape errors registered on {{ $labels.instance }} {{ $labels.mountpoint }}"
            - alert: MemoryUseHigh
              expr: (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes < 0.05) and (node_zfs_arc_size < node_memory_MemTotal_bytes / 20)
              for: 5m
              annotations:
                summary: "Memory usage > 95%"
                description: "See https://control.sufficientvelocity.com/grafana/d/9CWBz0bik/machine-stats"
          - name: systemd
            rules:
            - alert: UnitFailure
              expr: node_systemd_unit_state{name=~"(apt-daily-upgrade|nginx|php7.4-fpm|tinyproxy).service",state="failed"} > 0
              for: 5m
              annotations:
                summary: "Systemd unit failed"
                description: "{{ $labels.instance }} {{ $labels.name }}"
          - name: backups
            rules:
            - alert: LogEntriesExist
              expr: increase(zrepl_daemon_log_entries{level=~'warn|error'}[2h]) > 0
              annotations:
                summary: "All is not well with zrepl, log entries exist and should not."
                description: "See journalctl -u zrepl on {{ $labels.instance }}."
        ''
      ];
      exporters = {
        node = {
          enable = true;
          enabledCollectors =
            ["interrupts" "logind" "meminfo_numa" "mountstats" "tcpstat" "systemd" "zfs" "wifi" "textfile"]
            ++ (lib.optional config.services.ntp.enable "ntp");
          extraFlags = ["--collector.textfile.directory=/run/prometheus-node-exporter/"];
        };
        collectd.enable = true;
        nginx.enable = config.services.nginx.enable;
        postfix.enable = config.services.postfix.enable;
      };
      scrapeConfigs = [
        {
          job_name = "node";
          static_configs = [
            {
              targets = ["${config.networking.hostName}:${builtins.toString config.services.prometheus.exporters.node.port}"];
            }
          ];
        }
      ];
      globalConfig = {
        evaluation_interval = "10s";
        scrape_interval = "10s";
      };
    };
  };
}
