{ config, lib, ... }:

let
  cfg = config.me.mdns;
in
{
  options.me.mdns = {
    enable = lib.mkEnableOption "mDNS/DNS-SD via Avahi";
    publish = lib.mkEnableOption "mDNS address publishing (advertises this machine on the local network)";
  };

  config = lib.mkIf cfg.enable {
    services.avahi = {
      enable = true;
      nssmdns4 = true;
      publish = lib.mkIf cfg.publish {
        enable = true;
        addresses = true;
      };
    };

    # Disable resolved's built-in mDNS to avoid conflicts with Avahi.
    services.resolved.settings.Resolve.MulticastDNS = "no";
  };
}
