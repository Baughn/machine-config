{ lib, pkgs, ... }:

let
  victronMonitor = pkgs.callPackage ../../tools/victron-monitor { };
  prometheusPort = 9101;
  prefixUpdater = pkgs.writeScript "victron-starlink-prefixes" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings [ "@nft@" ] [ "${pkgs.nftables}/bin/nft" ]
      (builtins.readFile ./starlink-prefixes.py)
  );
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
      MemoryMax = "128M";
      TasksMax = 16;
    };
  };

  # The nft input hook filters before the NixOS firewall's loopback and
  # established-connection exceptions, including packets from local users.
  networking.firewall = {
    allowedUDPPorts = [ 9099 ];
    extraCommands = ''
      ${prefixUpdater} --restore
    '';
    # Keep the independent nft guard across stop/reload, including the old
    # firewall's loopback/established allowances. Replacement is atomic.
  };

  systemd.services.victron-starlink-prefixes = {
    description = "Refresh Starlink's announced prefixes for Victron ingress";
    after = [ "network-online.target" "firewall.service" ];
    wants = [ "network-online.target" ];
    environment.SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    serviceConfig = {
      Type = "oneshot";
      ExecStart = prefixUpdater;
      StateDirectory = "victron-prefixes";
      StateDirectoryMode = "0700";
      UMask = "0077";
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      CapabilityBoundingSet = [ "CAP_NET_ADMIN" ];
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" "AF_NETLINK" ];
      TimeoutStartSec = "90s";
      MemoryMax = "128M";
    };
  };
  systemd.timers.victron-starlink-prefixes = {
    wantedBy = [ "timers.target" ];
    timerConfig = {
      OnBootSec = "30s";
      OnUnitActiveSec = "1h";
      RandomizedDelaySec = "30s";
    };
  };

  services.prometheus.scrapeConfigs = lib.mkAfter [{
    job_name = "victron-monitor";
    static_configs = [{
      targets = [ "127.0.0.1:${toString prometheusPort}" ];
    }];
  }];
}
