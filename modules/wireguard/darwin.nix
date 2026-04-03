{ config, lib, pkgs, ... }:

let
  cfg = config.me.wireguard;
in
{
  options.me.wireguard = {
    enable = lib.mkEnableOption "WireGuard VPN";

    address = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      description = "This machine's WireGuard addresses with prefix length (e.g. [\"10.100.0.1/24\"])";
    };

    privateKeyFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to the WireGuard private key file (must not be in the nix store)";
    };

    listenPort = lib.mkOption {
      type = lib.types.nullOr lib.types.port;
      default = null;
      description = "UDP port to listen on. Set for hub machines that accept incoming connections.";
    };

    peers = lib.mkOption {
      type = lib.types.listOf (lib.types.submodule {
        options = {
          publicKey = lib.mkOption { type = lib.types.str; };
          allowedIPs = lib.mkOption { type = lib.types.listOf lib.types.str; };
          endpoint = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            description = "host:port — set when this machine initiates the connection";
          };
          persistentKeepalive = lib.mkOption {
            type = lib.types.nullOr lib.types.int;
            default = null;
            description = "Seconds between keepalive packets. Set for peers behind NAT.";
          };
        };
      });
      default = [];
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ pkgs.wireguard-tools ];
  };
}
