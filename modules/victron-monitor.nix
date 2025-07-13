{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.me.victron-monitor;

  victron-monitor-pkg = pkgs.callPackage ../tools/victron-monitor { };

  configFile = pkgs.writeText "victron-monitor-config.toml" ''
    udp_port = ${toString cfg.udpPort}
    prometheus_port = ${toString cfg.prometheusPort}
    
    # Device type mappings
    [device_mappings]
    ${concatStringsSep "\n" (mapAttrsToList (k: v: ''"${k}" = "${v}"'') cfg.deviceMappings)}
    
    # Unit mappings (from Victron notation to Prometheus suffix)
    [unit_mappings]
    ${concatStringsSep "\n" (mapAttrsToList (k: v: ''"${k}" = "${v}"'') cfg.unitMappings)}
  '';
in
{
  options.me.victron-monitor = {
    enable = mkOption {
      type = types.bool;
      default = false;
      description = "Enable Victron Energy monitoring bridge to Prometheus";
    };

    udpPort = mkOption {
      type = types.int;
      default = 9099;
      description = "UDP port to listen for incoming JSON data from Node-RED";
    };

    prometheusPort = mkOption {
      type = types.int;
      default = 9101;
      description = "HTTP port to expose Prometheus metrics";
    };

    deviceMappings = mkOption {
      type = types.attrsOf types.str;
      default = {
        "MultiPlus-II" = "inverter";
        "CAN-SMARTBMS-BAT" = "battery";
        "SmartSolar MPPT" = "charger";
      };
      description = "Mapping from Victron device types to metric prefixes";
    };

    unitMappings = mkOption {
      type = types.attrsOf types.str;
      default = {
        "W" = "watts";
        "A" = "amps";
        "V" = "volts";
        "V DC" = "volts_dc";
        "VAC" = "volts_ac";
        "%" = "percent";
        "Ah" = "amp_hours";
        "kWh" = "kilowatt_hours";
      };
      description = "Mapping from Victron units to Prometheus metric suffixes";
    };
  };

  config = mkIf cfg.enable {
    # Configure the systemd service
    systemd.services.victron-monitor = {
      description = "Victron Energy Monitoring Bridge to Prometheus";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart = "${victron-monitor-pkg}/bin/victron-monitor --config ${configFile}";
        Restart = "always";
        RestartSec = "10";

        # Security settings
        DynamicUser = true;

        # Hardening
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

    # Firewall configuration
    networking.firewall = {
      allowedUDPPorts = [ cfg.udpPort ];
    };
  };
}
