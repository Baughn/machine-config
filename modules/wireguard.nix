{ config, lib, pkgs, options, ... }:
let
  keys = import ./keys.nix;
  allBaseConfigs = builtins.concatLists (
    lib.mapAttrsToList (n: v: v.wireguard or [ ]) keys
  );
  allConfigs = builtins.map
    (v: {
      PublicKey = v.publicKey;
      AllowedIPs = [ ("10.171.0." + (toString v.id) + "/32") ];
      # Add endpoint for tsugumi server when configuring clients
      Endpoint = lib.mkIf (config.networking.hostName != "tsugumi") "tsugumi.local:51820";
      # Keep connection alive.
      PersistentKeepalive = lib.mkIf (config.networking.hostName == "tsugumi") 25;
    })
    allBaseConfigs;

  # Determine the IP address for this host based on machine-specific mapping
  hostToMachine = {
    tsugumi = "tsugumi-machine";
    saya = "saya-machine";
  };

  currentMachine = hostToMachine.${config.networking.hostName} or null;
  currentMachineKeys = if currentMachine != null then keys.${currentMachine} or null else null;
  currentMachineId =
    if currentMachineKeys != null && currentMachineKeys.wireguard != null
    then (builtins.head currentMachineKeys.wireguard).id
    else null;

  hostIp = toString currentMachineId;
in
{
  options.me.wireguard.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = "Enable custom WireGuard VPN configuration";
  };

  config = lib.mkIf config.me.wireguard.enable {
    # Ensure we have the wireguard secret
    age.secrets."wireguard/${config.networking.hostName}" = {
      file = ../secrets/wireguard-${config.networking.hostName}.age;
      mode = "0400";
      owner = "systemd-network";
      group = "systemd-network";
    };

    # WireGuard network device configuration
    systemd.network.netdevs."50-wg0" = {
      enable = true;
      netdevConfig = {
        Kind = "wireguard";
        Name = "wg0";
        MTUBytes = "1350";
      };
      wireguardConfig = {
        ListenPort = 51820;
        PrivateKeyFile = config.age.secrets."wireguard/${config.networking.hostName}".path;
      };
      wireguardPeers = allConfigs;
    };

    # WireGuard network configuration
    systemd.network.networks.wg0 = {
      enable = true;
      matchConfig.Name = "wg0";
      address = lib.mkIf (currentMachineId != null) [ "10.171.0.${hostIp}/24" ];
      networkConfig = {
        MulticastDNS = true;
      };
    };

    # Firewall configuration for WireGuard
    networking.firewall = {
      allowedUDPPorts = [ 51820 ]; # WireGuard port
      # Allow all traffic on the WireGuard interface
      trustedInterfaces = [ "wg0" ];
    };

    # Enable systemd-networkd if not already enabled
    systemd.network.enable = true;
  };
}
