{ lib, pkgs, ... }:

let
  victronMonitor = pkgs.callPackage ../../tools/victron-monitor { };
  prometheusPort = 9101;
  configFile = pkgs.writeText "victron-monitor-config.toml" ''
    udp_port = 9099
    prometheus_port = ${toString prometheusPort}

    [device_mappings]
    "MultiPlus-II" = "inverter"
    "CAN-SMARTBMS-BAT" = "battery"
    "SmartSolar MPPT" = "charger"

    [unit_mappings]
    "W" = "watts"
    "A" = "amps"
    "V" = "volts"
    "V DC" = "volts_dc"
    "VAC" = "volts_ac"
    "%" = "percent"
    "Ah" = "amp_hours"
    "kWh" = "kilowatt_hours"
  '';
in
{
  systemd.services.victron-monitor = {
    description = "Victron Energy Monitoring Bridge to Prometheus";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      ExecStart = "${victronMonitor}/bin/victron-monitor --config ${configFile}";
      Restart = "always";
      RestartSec = "10";
      DynamicUser = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      NoNewPrivileges = true;
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
      ProtectKernelTunables = true;
      ProtectKernelModules = true;
      ProtectControlGroups = true;
      RestrictNamespaces = true;
      LockPersonality = true;
      MemoryDenyWriteExecute = true;
      RestrictRealtime = true;
      RestrictSUIDSGID = true;
      RemoveIPC = true;
      PrivateTmp = true;
    };
  };

  networking.firewall.allowedUDPPorts = [ 9099 ];

  services.prometheus.scrapeConfigs = lib.mkAfter [{
    job_name = "victron-monitor";
    static_configs = [{
      targets = [ "127.0.0.1:${toString prometheusPort}" ];
    }];
  }];
}
