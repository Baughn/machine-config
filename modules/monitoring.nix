{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.me.monitoring;
in
{
  options.me.monitoring = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = "Enable monitoring stack (Prometheus, Grafana, Alertmanager)";
    };

    grafanaPort = mkOption {
      type = types.int;
      default = 1230;
      description = "Port for Grafana web interface";
    };

    prometheusPort = mkOption {
      type = types.int;
      default = 9090;
      description = "Port for Prometheus web interface";
    };

    alertmanagerPort = mkOption {
      type = types.int;
      default = 9093;
      description = "Port for Alertmanager web interface";
    };
  };

  config = mkIf cfg.enable {
    # Prometheus configuration
    services.prometheus = {
      enable = true;
      port = cfg.prometheusPort;
      listenAddress = "127.0.0.1";
      retention = "15d";
      
      scrapeConfigs = [
        {
          job_name = "prometheus";
          static_configs = [
            {
              targets = [ "127.0.0.1:${toString cfg.prometheusPort}" ];
            }
          ];
        }
        {
          job_name = "node";
          static_configs = [
            {
              targets = [ "127.0.0.1:${toString config.services.prometheus.exporters.node.port}" ];
            }
          ];
        }
        {
          job_name = "alertmanager";
          static_configs = [
            {
              targets = [ "127.0.0.1:${toString cfg.alertmanagerPort}" ];
            }
          ];
        }
        {
          job_name = "grafana";
          static_configs = [
            {
              targets = [ "127.0.0.1:${toString cfg.grafanaPort}" ];
            }
          ];
        }
      ];

      # Basic alerting rules
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
              annotations:
                summary: "High system load on {{ $labels.instance }}"
                description: "System load is {{ $value }}"
            
            - alert: DiskSpaceLow
              expr: (node_filesystem_avail_bytes * 100) / node_filesystem_size_bytes < 10
              for: 2m
              labels:
                severity: warning
              annotations:
                summary: "Low disk space on {{ $labels.instance }}"
                description: "Disk space is {{ $value }}% full"
            
            - alert: MemoryUsageHigh
              expr: (node_memory_MemTotal_bytes - node_memory_MemAvailable_bytes) / node_memory_MemTotal_bytes > 0.9
              for: 5m
              labels:
                severity: warning
              annotations:
                summary: "High memory usage on {{ $labels.instance }}"
                description: "Memory usage is {{ $value | humanizePercentage }}"
            
            - alert: ServiceDown
              expr: up == 0
              for: 1m
              labels:
                severity: critical
              annotations:
                summary: "Service {{ $labels.job }} down"
                description: "{{ $labels.job }} on {{ $labels.instance }} is down"
        ''
      ];

      alertmanager = {
        enable = true;
        port = cfg.alertmanagerPort;
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
          
          receivers = [
            {
              name = "default";
              # Future: add webhook notifications here
            }
          ];
        };
      };
    };

    # Node exporter for system metrics
    services.prometheus.exporters.node = {
      enable = true;
      enabledCollectors = [
        "systemd"
        "filesystem"
        "network"
        "memory"
        "cpu"
        "loadavg"
        "diskstats"
        "netdev"
        "meminfo"
        "stat"
      ];
      port = 9100;
      listenAddress = "127.0.0.1";
      # Don't expose on firewall - only internal access
      openFirewall = false;
    };

    # Grafana configuration
    services.grafana = {
      enable = true;
      settings = {
        server = {
          http_port = cfg.grafanaPort;
          http_addr = "127.0.0.1";
          domain = "grafana.brage.info";
          root_url = "https://grafana.brage.info/";
        };
        
        # Security settings
        security = {
          admin_user = "admin";
          admin_password = "$__file{${config.age.secrets."grafana-admin-password".path}}";
          disable_gravatar = true;
        };
        
        # Anonymous access disabled - use Authelia
        auth.anonymous = {
          enabled = false;
        };
        
        # Disable user signup
        users = {
          allow_sign_up = false;
          allow_org_create = false;
          auto_assign_org = true;
          auto_assign_org_role = "Viewer";
        };
        
        # Analytics disabled
        analytics = {
          reporting_enabled = false;
          check_for_updates = false;
        };
      };
      
      # Declarative datasource provisioning
      provision = {
        enable = true;
        datasources.settings.datasources = [
          {
            name = "Prometheus";
            type = "prometheus";
            access = "proxy";
            url = "http://127.0.0.1:${toString cfg.prometheusPort}";
            isDefault = true;
          }
        ];
        
        # Basic system dashboard
        dashboards.settings.providers = [
          {
            name = "default";
            type = "file";
            options.path = pkgs.writeTextDir "system-overview.json" (builtins.toJSON {
              dashboard = {
                id = null;
                title = "System Overview";
                tags = [ "system" ];
                timezone = "browser";
                panels = [
                  {
                    id = 1;
                    title = "System Load";
                    type = "graph";
                    targets = [
                      {
                        expr = "node_load1";
                        legendFormat = "1m load";
                      }
                      {
                        expr = "node_load5";
                        legendFormat = "5m load";
                      }
                      {
                        expr = "node_load15";
                        legendFormat = "15m load";
                      }
                    ];
                    gridPos = { h = 8; w = 12; x = 0; y = 0; };
                  }
                  {
                    id = 2;
                    title = "Memory Usage";
                    type = "graph";
                    targets = [
                      {
                        expr = "node_memory_MemTotal_bytes - node_memory_MemAvailable_bytes";
                        legendFormat = "Used";
                      }
                      {
                        expr = "node_memory_MemAvailable_bytes";
                        legendFormat = "Available";
                      }
                    ];
                    gridPos = { h = 8; w = 12; x = 12; y = 0; };
                  }
                  {
                    id = 3;
                    title = "Disk Usage";
                    type = "graph";
                    targets = [
                      {
                        expr = "node_filesystem_size_bytes - node_filesystem_avail_bytes";
                        legendFormat = "Used - {{ mountpoint }}";
                      }
                    ];
                    gridPos = { h = 8; w = 24; x = 0; y = 8; };
                  }
                ];
                time = {
                  from = "now-1h";
                  to = "now";
                };
                refresh = "5s";
              };
            });
          }
        ];
      };
    };

    # Firewall configuration for internal monitoring ports
    networking.firewall = {
      interfaces.lo.allowedTCPPorts = [
        cfg.grafanaPort
        cfg.prometheusPort  
        cfg.alertmanagerPort
        config.services.prometheus.exporters.node.port
      ];
    };

    # Systemd service ordering
    systemd.services.prometheus.wants = [ "prometheus-node-exporter.service" ];
    systemd.services.prometheus.after = [ "prometheus-node-exporter.service" ];
    systemd.services.grafana.wants = [ "prometheus.service" ];
    systemd.services.grafana.after = [ "prometheus.service" ];
  };
}