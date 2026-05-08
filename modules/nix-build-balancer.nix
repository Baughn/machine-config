{ config, lib, pkgs, ... }:

let
  cfg = config.me.nixBuildBalancer;
  package = pkgs.callPackage ../tools/nix-build-balancer/default.nix { };
  endpoint = "unix:${cfg.unixSocket}";

  remoteArgs =
    lib.concatLists
      (lib.mapAttrsToList
        (name: addr: [ "--remote" "${name}=${addr}" ])
        cfg.remoteAgents);

  serveArgs = [
    "serve"
    "--mode" cfg.mode
    "--host" config.networking.hostName
    "--data-dir" cfg.dataDir
    "--poll-interval-ms" (toString cfg.pollIntervalMs)
    "--max-samples-per-pname" (toString cfg.maxSamplesPerPname)
    "--stale-start-ms" (toString cfg.staleStartMs)
  ]
  ++ lib.optionals (cfg.unixSocket != null) [ "--unix-socket" cfg.unixSocket ]
  ++ lib.optionals (cfg.listenAddress != null) [ "--listen" cfg.listenAddress ]
  ++ remoteArgs;

  eventArgs = kind: [
    "event"
    "--endpoint" endpoint
    "--kind" kind
    "--host" config.networking.hostName
  ];

  preBuildHook = pkgs.writeShellScript "nix-build-balancer-pre-build-hook" ''
    set +e
    drv_path="''${1:-}"
    if [ -n "$drv_path" ]; then
      ${lib.escapeShellArgs ([ "${package}/bin/nix-build-balancer" ] ++ eventArgs "start" ++ [ "--drv-path" ])} "$drv_path" >/dev/null 2>&1
    fi
    exit 0
  '';

  postBuildHook = pkgs.writeShellScript "nix-build-balancer-post-build-hook" ''
    set +e
    if [ -n "''${DRV_PATH:-}" ]; then
      ${lib.escapeShellArgs ([ "${package}/bin/nix-build-balancer" ] ++ eventArgs "finish" ++ [ "--drv-path" ])} "$DRV_PATH" \
        --out-paths "''${OUT_PATHS:-}" \
        --status success >/dev/null 2>&1
    fi
    exit 0
  '';

  schedulerHook = pkgs.writeShellScript "nix-build-balancer-build-hook" ''
    exec ${lib.escapeShellArgs [
      "${package}/bin/nix-build-balancer"
      "hook"
      "--endpoint" endpoint
      "--host" config.networking.hostName
      "--remote-host" cfg.scheduler.remoteHost
      "--remote-store-uri" cfg.scheduler.remoteStoreUri
      "--remote-builder" cfg.scheduler.remoteBuilder
      "--nix-bin" "${config.nix.package}/bin/nix"
      "--"
    ]} "$@"
  '';
in
{
  options.me.nixBuildBalancer = {
    enable = lib.mkEnableOption "Nix build telemetry daemon";

    mode = lib.mkOption {
      type = lib.types.enum [ "agent" "controller" ];
      default = "agent";
      description = "Whether this host only exports telemetry or also polls remote agents.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/nix-build-balancer";
      description = "Persistent daemon state directory.";
    };

    unixSocket = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = "/run/nix-build-balancer/balancer.sock";
      description = "Local Unix socket used by Nix hooks.";
    };

    listenAddress = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "10.171.0.1:8765";
      description = "Optional TCP listen address for remote telemetry polling.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the TCP port from listenAddress in the host firewall.";
    };

    remoteAgents = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = { tsugumi = "10.171.0.1:8765"; };
      description = "Remote agent addresses polled by controller mode.";
    };

    pollIntervalMs = lib.mkOption {
      type = lib.types.ints.positive;
      default = 1000;
      description = "Telemetry polling interval for controller mode.";
    };

    maxSamplesPerPname = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 200;
      description = "Maximum retained completed build observations per pname. Set to 0 to disable pruning.";
    };

    staleStartMs = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 24 * 60 * 60 * 1000;
      description = "Age in milliseconds after which unmatched build starts are removed. Set to 0 to disable cleanup.";
    };

    installNixHooks = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install best-effort Nix pre/post build observation hooks.";
    };

    scheduler = {
      enable = lib.mkEnableOption "custom Nix remote build scheduler hook";

      remoteHost = lib.mkOption {
        type = lib.types.str;
        default = "tsugumi";
        description = "Remote telemetry/admission host name used by the scheduler.";
      };

      remoteStoreUri = lib.mkOption {
        type = lib.types.str;
        default = "ssh-ng://svein@tsugumi.local";
        description = "Remote store URI returned by accepted scheduler decisions.";
      };

      remoteBuilder = lib.mkOption {
        type = lib.types.str;
        default = "ssh-ng://svein@tsugumi.local x86_64-linux /home/svein/.ssh/id_ed25519 16 1 nixos-test,kvm,big-parallel - -";
        description = "Single Nix machine line passed to stock nix __build-remote for accepted builds.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0755 root root -"
    ]
    ++ lib.optionals (cfg.unixSocket != null) [
      "d ${dirOf cfg.unixSocket} 0755 root root -"
    ];

    systemd.services.nix-build-balancer = {
      description = "Nix build telemetry daemon";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "simple";
        ExecStart = lib.escapeShellArgs ([ "${package}/bin/nix-build-balancer" ] ++ serveArgs);
        Restart = "on-failure";
        RestartSec = "2s";
        StateDirectory = "nix-build-balancer";
        RuntimeDirectory = "nix-build-balancer";
      };
    };

    environment.systemPackages = [ package ];

    nix.settings =
      (lib.optionalAttrs cfg.installNixHooks {
        pre-build-hook = preBuildHook;
        post-build-hook = postBuildHook;
      })
      // (lib.optionalAttrs cfg.scheduler.enable {
        build-hook = schedulerHook;
      });

    networking.firewall.allowedTCPPorts =
      lib.optionals (cfg.openFirewall && cfg.listenAddress != null)
        [ (lib.toInt (lib.last (lib.splitString ":" cfg.listenAddress))) ];
  };
}
