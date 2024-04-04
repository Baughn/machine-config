{
  config,
  lib,
  ...
}: let
  keys = import ./keys.nix;
  allBaseConfigs = builtins.concatLists (
    lib.mapAttrsToList (n: v: v.wireguard or []) keys);
  # Get a list of every unique client (by public key).
  allKeys = builtins.map (c: c.publicKey) allBaseConfigs;
  # Generate a unique IP per client.
  allIPs = lib.genList (i: "10.171.0.${builtins.toString (i+2)}") (builtins.length allKeys);
  # Zip the keys and IPs together.
  allConfigs = lib.zipListsWith (key: ip: {
    wireguardPeerConfig = {
      PublicKey = key;
      AllowedIPs = [ip];
    };
  }) allKeys allIPs;
in {
  systemd.network.netdevs = {
    "50-wg0" = {
      enable = true;
      netdevConfig = {
        Kind = "wireguard";
        Name = "wg0";
        MTUBytes = "1350";
      };
      wireguardConfig = {
        ListenPort = 51820;
        PrivateKeyFile = config.age.secrets."wireguard/tsugumi".path;
      };
      wireguardPeers = allConfigs;
    };
  };
  systemd.network.networks.wg0 = {
    enable = true;
    matchConfig.Name = "wg0";
    address = ["10.171.0.1/24"];
  };
}
