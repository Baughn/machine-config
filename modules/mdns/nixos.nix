{ config, lib, ... }:

let
  cfg = config.me.mdns;
in
{
  options.me.mdns = {
    enable = lib.mkEnableOption "mDNS/DNS-SD via Avahi";
  };

  config = lib.mkIf cfg.enable {
    services.avahi = {
      enable = true;
      nssmdns4 = true;
      publish = {
        enable = true;
        addresses = true;
      };
    };

    # Disable resolved's built-in mDNS to avoid conflicts with Avahi.
    services.resolved.settings.Resolve.MulticastDNS = "no";
  };
}
