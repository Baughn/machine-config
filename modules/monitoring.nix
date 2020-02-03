{ config, pkgs, lib, ... }:

{
  services.prometheus = {
    enable = true;
    alertmanager = {
      enable = true;  # Make dependent on host. Just run one on Tsugumi.
      configuration = {
        global = {
          smtp_from = "prometheus@" + config.networking.hostName;
          smtp_smarthost = "localhost:25";
        };
        route = {
          receiver = "default-receiver";
          group_wait = "30s";
          group_interval = "5m";
          repeat_interval = "24h";
          group_by = ["alertname"];
        };
        receivers = [{
          name = "default-receiver";
          email_configs = [{
            to = "<svein@localhost>";  # Expose as configuration option.
            require_tls = false;
          }];
        }];
      };
    };
    alertmanagerURL = [("http://localhost:" + builtins.toString config.services.prometheus.alertmanager.port)];
    rules = [
      ''ALERT JobDown IF up == 0 FOR 5m''
      ''ALERT DiskspaceLow IF node_filesystem_free_bytes{fstype!~"tmpfs|ramfs"} / node_filesystem_size_bytes < 0.15 FOR 5m''
      ''ALERT MemoryLow IF node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes < 0.15 FOR 5m''
      ''ALERT SystemdUnitFailed IF node_systemd_units{state="failed"} > 0 FOR 5m''
    ];
    exporters = {
      node = {
        enable = true;
        enabledCollectors = [ "interrupts" "logind" "meminfo_numa" "mountstats" "systemd" "tcpstat" ] ++
          (lib.optional config.services.ntp.enable "ntp");
      };
      collectd.enable = true;
      nginx.enable = config.services.nginx.enable;
      postfix.enable = config.services.postfix.enable;
    };
    scrapeConfigs = (lib.mapAttrsToList (name: val: {
      job_name = name;
      static_configs = [{
        targets = [("localhost:" + (builtins.toString val.port))];
      }];
    }) (lib.filterAttrs (n: v: v.enable or false) config.services.prometheus.exporters)) ++ [{
      job_name = "prometheus";
      static_configs = [{
        targets = [ "localhost:9090" ];
      }];
    }];
    globalConfig = {
      evaluation_interval = "1m";
      scrape_interval = "1m";
    };
  };
}
