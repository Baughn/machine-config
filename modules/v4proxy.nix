{ config, pkgs, lib, ... }:

let
  cfg = config.services.v4proxy;

  mappingType = lib.types.submodule {
    options = {
      protocol = lib.mkOption {
        type = lib.types.enum [ "tcp" "udp" ];
        default = "tcp";
        description = "Protocol to proxy (tcp or udp)";
      };
      localPort = lib.mkOption {
        type = lib.types.port;
        description = "Local port to listen on";
      };
      remotePort = lib.mkOption {
        type = lib.types.nullOr lib.types.port;
        default = null;
        description = "Remote port to connect to (defaults to localPort if not specified)";
      };
      target = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Target hostname (defaults to defaultTarget if not specified)";
      };
    };
  };

  # Convert a mapping to CLI format: protocol:localPort[:remotePort][@target]
  mappingToString = m:
    let
      remotePort = if m.remotePort != null then m.remotePort else m.localPort;
      target = if m.target != null then m.target else cfg.defaultTarget;
      portPart =
        if remotePort != m.localPort
        then "${toString m.localPort}:${toString remotePort}"
        else toString m.localPort;
    in
    "${m.protocol}:${portPart}@${target}";

  mappingsString = lib.concatStringsSep "," (map mappingToString cfg.mappings);

  # Extract ports by protocol for firewall
  tcpPorts = map (m: m.localPort) (lib.filter (m: m.protocol == "tcp") cfg.mappings);
  udpPorts = map (m: m.localPort) (lib.filter (m: m.protocol == "udp") cfg.mappings);
in

{
  options.services.v4proxy = {
    enable = lib.mkEnableOption "IPv4 to IPv6 proxy service";

    defaultTarget = lib.mkOption {
      type = lib.types.str;
      default = "direct.brage.info";
      description = "Default target hostname when not specified per-mapping";
    };

    mappings = lib.mkOption {
      type = lib.types.listOf mappingType;
      default = [ ];
      description = "List of port mappings to proxy";
      example = lib.literalExpression ''
        [
          { localPort = 25565; }                    # TCP, default target
          { localPort = 25566; }                    # TCP, default target
          { protocol = "udp"; localPort = 24454; } # UDP voice chat
        ]
      '';
    };

    timeout = lib.mkOption {
      type = lib.types.int;
      default = 30;
      description = "TCP connection timeout in seconds";
    };

    udpSessionTimeout = lib.mkOption {
      type = lib.types.int;
      default = 60;
      description = "UDP session timeout in seconds (inactive sessions are cleaned up)";
    };

    bufferSize = lib.mkOption {
      type = lib.types.int;
      default = 8192;
      description = "Buffer size for data transfer in bytes";
    };
  };

  config = lib.mkIf cfg.enable {
    # Build the v4proxy package from source
    nixpkgs.overlays = [
      (self: _: {
        minecraft-ipv6-proxy = self.rustPlatform.buildRustPackage {
          pname = "minecraft-ipv6-proxy";
          version = "0.1.0";
          src = ../machines/v4/v4proxy;
          cargoHash = "sha256-ZqrgZtcKR81BTE6Qdt3TbltuKURu6uwWB6LDIyd3VfA=";

          meta = with lib; {
            description = "IPv4 to IPv6 proxy for servers (TCP and UDP)";
            platforms = platforms.linux;
          };
        };
      })
    ];

    # Configure the systemd service
    systemd.services.minecraft-ipv6-proxy = {
      description = "IPv4 to IPv6 Proxy";
      after = [ "network.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart = lib.concatStringsSep " " [
          "${pkgs.minecraft-ipv6-proxy}/bin/v4proxy"
          "--mappings '${mappingsString}'"
          "--default-target ${cfg.defaultTarget}"
          "--timeout ${toString cfg.timeout}"
          "--udp-session-timeout ${toString cfg.udpSessionTimeout}"
          "--buffer-size ${toString cfg.bufferSize}"
        ];
        Restart = "always";
        RestartSec = "10";

        # Security settings
        DynamicUser = true;
        AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
        CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];

        # Hardening
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
      };
    };

    networking.firewall.allowedTCPPorts = tcpPorts;
    networking.firewall.allowedUDPPorts = udpPorts;
  };
}
