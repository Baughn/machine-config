{ ... }:

let
  prometheusPort = 9090;
  alertmanagerPort = 9093;
  nodeExporterPort = 9100;
in
{
  services.prometheus = {
    enable = true;
    port = prometheusPort;
    listenAddress = "127.0.0.1";
    retentionTime = "30d";
    scrapeConfigs = [
      {
        job_name = "prometheus";
        static_configs = [{ targets = [ "127.0.0.1:${toString prometheusPort}" ]; }];
      }
      {
        job_name = "node";
        static_configs = [{ targets = [ "127.0.0.1:${toString nodeExporterPort}" ]; }];
      }
      {
        job_name = "alertmanager";
        static_configs = [{ targets = [ "127.0.0.1:${toString alertmanagerPort}" ]; }];
      }
    ];
    rules = [
      ''
        groups:
        - name: system
          rules:
          - alert: SystemLoad
            expr: node_load1 > 0.8
            for: 5m
            labels:
              severity: warning
          - alert: DiskSpaceLow
            expr: (node_filesystem_avail_bytes * 100) / node_filesystem_size_bytes < 10
            for: 2m
            labels:
              severity: warning
          - alert: MemoryUsageHigh
            expr: (node_memory_MemTotal_bytes - node_memory_MemAvailable_bytes) / node_memory_MemTotal_bytes > 0.9
            for: 5m
            labels:
              severity: warning
          - alert: ServiceDown
            expr: up == 0
            for: 1m
            labels:
              severity: critical
      ''
    ];
    alertmanager = {
      enable = true;
      port = alertmanagerPort;
      listenAddress = "127.0.0.1";
      configuration = {
        global = {
          smtp_smarthost = "localhost:587";
          smtp_from = "alertmanager@brage.info";
        };
        route = {
          group_by = [ "alertname" ];
          group_wait = "10s";
          group_interval = "10s";
          repeat_interval = "1h";
          receiver = "default";
        };
        receivers = [{ name = "default"; }];
      };
    };
    exporters.node = {
      enable = true;
      enabledCollectors = [
        "systemd"
        "filesystem"
        "netdev"
        "meminfo"
        "cpu"
        "loadavg"
        "diskstats"
        "stat"
      ];
      port = nodeExporterPort;
      listenAddress = "127.0.0.1";
      openFirewall = false;
    };
  };

  networking.firewall.interfaces.lo.allowedTCPPorts = [
    prometheusPort
    alertmanagerPort
    nodeExporterPort
  ];

  systemd.services.prometheus = {
    wants = [ "prometheus-node-exporter.service" ];
    after = [ "prometheus-node-exporter.service" ];
  };
}
