{ config, pkgs, ... }:

let
  grafanaPort = 1230;
  prometheusPort = 9090;
  alertmanagerPort = 9093;
  nodeExporterPort = 9100;
in
{
  services.prometheus = {
    enable = true;
    port = prometheusPort;
    listenAddress = "127.0.0.1";
    retentionTime = "15d";
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
      {
        job_name = "grafana";
        static_configs = [{ targets = [ "127.0.0.1:${toString grafanaPort}" ]; }];
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

  services.grafana = {
    enable = true;
    settings = {
      server = {
        http_port = grafanaPort;
        http_addr = "127.0.0.1";
        domain = "grafana.brage.info";
        root_url = "https://grafana.brage.info/";
      };
      security = {
        admin_user = "admin";
        admin_password = "$__file{${config.age.secrets."grafana-admin-password".path}}";
        disable_gravatar = true;
        secret_key = "$__file{${config.age.secrets."grafana-admin-password".path}}";
      };
      "auth.anonymous".enabled = false;
      users = {
        allow_sign_up = false;
        allow_org_create = false;
        auto_assign_org = true;
        auto_assign_org_role = "Viewer";
      };
      analytics = {
        reporting_enabled = false;
        check_for_updates = false;
      };
    };
    provision = {
      enable = true;
      datasources.settings.datasources = [{
        name = "Prometheus";
        type = "prometheus";
        access = "proxy";
        url = "http://127.0.0.1:${toString prometheusPort}";
        isDefault = true;
      }];
      dashboards.settings.providers = [{
        name = "default";
        type = "file";
        options.path = pkgs.writeTextDir "system-overview.json" (builtins.toJSON {
          dashboard = {
            id = null;
            title = "System Overview";
            tags = [ "system" ];
            timezone = "browser";
            panels = [ ];
            time = {
              from = "now-1h";
              to = "now";
            };
            refresh = "5s";
          };
        });
      }];
    };
  };

  networking.firewall.interfaces.lo.allowedTCPPorts = [
    grafanaPort
    prometheusPort
    alertmanagerPort
    nodeExporterPort
  ];

  systemd.services.prometheus = {
    wants = [ "prometheus-node-exporter.service" ];
    after = [ "prometheus-node-exporter.service" ];
  };
  systemd.services.grafana = {
    wants = [ "prometheus.service" ];
    after = [ "prometheus.service" ];
  };
}
