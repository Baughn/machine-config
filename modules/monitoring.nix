{ config, pkgs, lib, ... }:

{
  systemd.services.alertmanager-discord = {
    requires = [ "network-online.target" ];
    after = [ "network-online.target" ];
    wantedBy = [ "multi-user.target" ];
    environment = {
      DISCORD_WEBHOOK = (import ../secrets).monitoringWebhook;
    };
    serviceConfig = {
      ExecStart = "${pkgs.callPackage monitoring/alertmanager-discord {}}/bin/alertmanager-discord --listen.address=127.0.0.1:9095";
      Restart = "always";
    };
  };
  systemd.services.prometheus-zpool-exporter = {
    wantedBy = [ "multi-user.target" ];
    script = ''
      DIR=/run/prometheus-node-exporter/
      while true; do
        echo > $DIR/zpool
        for pool in $(${pkgs.zfs}/bin/zpool list -H | ${pkgs.gawk}/bin/awk '{print $1}'); do
          if ${pkgs.zfs}/bin/zpool status -x | grep -q $pool; then
            echo "zfs_pool_errors{pool=\"$pool\"} 1" >> $DIR/zpool
          else
            echo "zfs_pool_errors{pool=\"$pool\"} 0" >> $DIR/zpool
          fi
        done
        mv $DIR/zpool $DIR/zpool.prom
        sleep 30
      done
    '';
  };

  services.prometheus = {
    enable = true;
    alertmanager = {
      enable = true;  # Make dependent on host. Just run one on Tsugumi.
      port = 9093;
      configuration = {
        route = {
          group_by = ["alertname"];
          group_wait = "30s";
          repeat_interval = "1h";
          receiver = "discord";
        };
        receivers = [{
          name = "discord";
          webhook_configs = [{
            send_resolved = true;
            url = "http://localhost:9095";
          }];
        }];
      };
    };
    alertmanagers = [{
      scheme = "http";
      #path_prefix = "/alertmanager";
      static_configs = [{
        targets = [ "localhost:${builtins.toString config.services.prometheus.alertmanager.port}" ];
      }];
    }];
    rules = [''
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
          expr: node_hwmon_temp_celsius > 86
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
          expr: node_filesystem_avail_bytes{fstype=~"ext4|zfs|xfs"} / node_filesystem_size_bytes < 0.1
          annotations:
            summary: "Filesystem space use > 90%"
            description: "S{{ $value }} (fraction) free on {{ $labels.instance }} {{ $labels.mountpoint }}"
        - alert: DiskFreeSpaceLow
          expr: node_filesystem_avail_bytes{fstype="zfs"} < 10000000000
          annotations:
            summary: "Too little free space left."
            description: "{{ $value }} bytes free on {{ $labels.instance }} {{ $labels.mountpoint }}"
            details: "Working around https://github.com/prometheus/node_exporter/issues/1498"
        - alert: FilesystemScrapeErrors
          expr: node_filesystem_device_error{mountpoint!~"/var/lib/unifi/.*"} > 0
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
        enabledCollectors = [ "interrupts" "logind" "meminfo_numa" "mountstats" "tcpstat" "systemd" "zfs" "wifi" "textfile" ] ++
          (lib.optional config.services.ntp.enable "ntp");
        extraFlags = ["--collector.textfile.directory=/run/prometheus-node-exporter/"];
      };
      collectd.enable = true;
      nginx.enable = config.services.nginx.enable;
      postfix.enable = config.services.postfix.enable;
    };
    scrapeConfigs = [{
      job_name = "node";
      static_configs = [{
        targets = ["localhost:${builtins.toString config.services.prometheus.exporters.node.port}"];
      }];
    }];
    globalConfig = {
      evaluation_interval = "10s";
      scrape_interval = "10s";
    };
  };
}
