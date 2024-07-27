{
  config,
  lib,
  ...
}: let
  keys = import ./keys.nix;
  allBaseConfigs = builtins.concatLists (
    lib.mapAttrsToList (n: v: v.wireguard or []) keys);
  allConfigs = builtins.map (v: {
    PublicKey = v.publicKey;
    AllowedIPs = [("10.171.0." + (toString v.id) + "/32")];
  }) allBaseConfigs;
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
    networkConfig.MulticastDNS = true;
  };
}
