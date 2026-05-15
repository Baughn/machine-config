{ config, lib, pkgs, ... }:

let
  cfg = config.me.nixBuildBalancer;
  package = pkgs.callPackage ../tools/nix-build-balancer/default.nix { };

  isController = cfg.role == "controller" || cfg.role == "both";
  isAgent = cfg.role == "agent" || cfg.role == "both";

  formatTarget = name: t:
    let
      base = "${name}=${t.tcpAddr}|${toString t.capacity}|${t.storeUri}|${t.builderLine}";
      flags =
        lib.optionalString t.isLocal "|is_local"
        + lib.optionalString (t.speedMultiplier != 1.0) "|speed=${toString t.speedMultiplier}";
    in base + flags;

  targetArgs =
    lib.concatLists
      (lib.mapAttrsToList (name: t: [ "--target" (formatTarget name t) ]) cfg.targets);

  controllerArgs = [
    "--system" cfg.system
    "--data-dir" "/var/lib/nbb"
    "--inflight-dir" "/run/nbb/inflight"
    "--hook-socket" "/run/nbb/decide.sock"
    "--poll-interval-ms" (toString cfg.pollIntervalMs)
    "--min-remote-mem-available-kb" (toString cfg.minRemoteMemAvailableKb)
    "--unknown-p95-ms" (toString cfg.unknownP95Ms)
    "--max-samples-per-pname" (toString cfg.maxSamplesPerPname)
    "--ewma-alpha" (toString cfg.ewmaAlpha)
    "--ewma-z" (toString cfg.ewmaZ)
  ] ++ targetArgs;

  agentArgs = [
    "--bind" cfg.agentListen
    "--spool-dir" "/var/lib/nbb/spool"
    "--hostname" config.networking.hostName
    "--system" cfg.system
    "--capacity" (toString cfg.agentCapacity)
  ];

  # Nix pre-build-hook invokes the binary directly. nbb-event is intentionally
  # tiny and fail-closed (any error → exit 0), so no shell wrapping is required.
  preBuildHook = pkgs.writeShellScript "nbb-pre-build-hook" ''
    drv_path="''${1:-}"
    if [ -n "$drv_path" ]; then
      ${package}/bin/nbb-event --kind start --drv-path "$drv_path" >/dev/null 2>&1 || true
    fi
    exit 0
  '';

  postBuildHook = pkgs.writeShellScript "nbb-post-build-hook" ''
    if [ -n "''${DRV_PATH:-}" ]; then
      ${package}/bin/nbb-event \
        --kind finish \
        --drv-path "$DRV_PATH" \
        --status success \
        --out-paths "''${OUT_PATHS:-}" >/dev/null 2>&1 || true
    fi
    exit 0
  '';
in
{
  options.me.nixBuildBalancer = {
    enable = lib.mkEnableOption "nix-build-balancer";

    role = lib.mkOption {
      type = lib.types.enum [ "controller" "agent" "both" ];
      default = "agent";
      description = "Whether this host runs the controller, an agent, or both.";
    };

    system = lib.mkOption {
      type = lib.types.str;
      default = "x86_64-linux";
      description = "Nix system this deployment routes for.";
    };

    agentListen = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0:8765";
      description = "TCP bind address for the agent's listener.";
    };

    agentCapacity = lib.mkOption {
      type = lib.types.ints.positive;
      default = 1;
      description = "Local build capacity reported by the agent in AGENT_HELLO.";
    };

    targets = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options = {
          tcpAddr = lib.mkOption {
            type = lib.types.str;
            example = "10.171.0.1:8765";
            description = "TCP endpoint of the target's nbb-agent.";
          };
          capacity = lib.mkOption {
            type = lib.types.ints.positive;
            description = "Maximum parallel builds the controller may queue against this target.";
          };
          storeUri = lib.mkOption {
            type = lib.types.str;
            example = "ssh-ng://svein@tsugumi.local";
          };
          builderLine = lib.mkOption {
            type = lib.types.str;
            description = ''
              Pre-formatted Nix `machines` line passed to `nix __build-remote`
              when this target wins. Unused for local targets (which always
              short-circuit to a Decline).
            '';
          };
          isLocal = lib.mkOption {
            type = lib.types.bool;
            default = false;
            description = "True if this target is the controller's own host.";
          };
          speedMultiplier = lib.mkOption {
            type = lib.types.float;
            default = 1.0;
            description = "Per-target speed multiplier; 1.0 today.";
          };
        };
      });
      default = { };
      description = "Routable build sites known to the controller.";
    };

    pollIntervalMs = lib.mkOption {
      type = lib.types.ints.positive;
      default = 1000;
      description = "Per-agent poll interval. Stale-PONG threshold is 3x this.";
    };

    minRemoteMemAvailableKb = lib.mkOption {
      type = lib.types.ints.positive;
      default = 1000000;
    };

    unknownP95Ms = lib.mkOption {
      type = lib.types.ints.positive;
      default = 60000;
      description = ''
        Fallback duration (ms) used when the controller has no
        observations for a pname yet.
      '';
    };

    maxSamplesPerPname = lib.mkOption {
      type = lib.types.ints.positive;
      default = 200;
      description = ''
        Per-pname observation cap; oldest rows are pruned beyond this
        count. The EWMA estimator still walks the surviving rows in
        chronological order.
      '';
    };

    ewmaAlpha = lib.mkOption {
      type = lib.types.float;
      default = 0.2;
      description = ''
        EWMA smoothing factor for the per-pname duration estimator
        (log-normal model in `src/estimator.rs`). Must be in (0, 1].
        Half-life in observations is ln(0.5)/ln(1 − α); the default
        0.2 → ≈3.1 obs.
      '';
    };

    ewmaZ = lib.mkOption {
      type = lib.types.float;
      default = 1.645;
      description = ''
        Standard-normal quantile read by the estimator;
        1.645 ≈ Φ⁻¹(0.95). Use 1.96 for ≈Φ⁻¹(0.975) if the scheduler
        should over-predict more.
      '';
    };

    installNixHooks = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install Nix pre-build-hook / post-build-hook calling nbb-event.";
    };

    scheduler.enable = lib.mkEnableOption "nbb-hook as Nix build-hook";

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the agent's TCP port in the firewall.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.tmpfiles.rules = [
      "d /var/lib/nbb 0755 root root -"
      "d /var/lib/nbb/spool 0755 root root -"
      "d /run/nbb 0755 root root -"
    ] ++ lib.optionals isController [
      "d /run/nbb/inflight 0755 root root -"
    ];

    systemd.services =
      (lib.optionalAttrs isController {
        nbb-controller = {
          description = "nix-build-balancer controller";
          wantedBy = [ "multi-user.target" ];
          after = [ "network-online.target" ];
          wants = [ "network-online.target" ];
          serviceConfig = {
            Type = "simple";
            ExecStart = lib.escapeShellArgs ([ "${package}/bin/nbb-controller" ] ++ controllerArgs);
            Restart = "on-failure";
            RestartSec = "2s";
            StateDirectory = "nbb";
            RuntimeDirectory = "nbb";
          };
        };
      }) // (lib.optionalAttrs isAgent {
        nbb-agent = {
          description = "nix-build-balancer agent";
          wantedBy = [ "multi-user.target" ];
          after = [ "network-online.target" ];
          wants = [ "network-online.target" ];
          serviceConfig = {
            Type = "simple";
            ExecStart = lib.escapeShellArgs ([ "${package}/bin/nbb-agent" ] ++ agentArgs);
            Restart = "on-failure";
            RestartSec = "2s";
            StateDirectory = "nbb";
          };
        };
      });

    environment.systemPackages = [ package ];

    nix.settings =
      (lib.optionalAttrs cfg.installNixHooks {
        pre-build-hook = preBuildHook;
        post-build-hook = postBuildHook;
      })
      // (lib.optionalAttrs (cfg.scheduler.enable && isController) {
        build-hook = "${package}/bin/nbb-hook";
      });

    networking.firewall.allowedTCPPorts =
      lib.optionals (isAgent && cfg.openFirewall) [
        (lib.toInt (lib.last (lib.splitString ":" cfg.agentListen)))
      ];
  };
}
