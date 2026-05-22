{ config, lib, ... }:

let
  cfg = config.me.mdns;
in
{
  options.me.mdns = {
    enable = lib.mkEnableOption "mDNS/DNS-SD via Avahi";
    publish = lib.mkEnableOption "mDNS address publishing (advertises this machine on the local network)";
  };

  # systemd-resolved handles all .local *resolution* (both A and AAAA, via its
  # native mDNS). Avahi stays only for browse/publish — DNS-SD service
  # discovery and announcing this host. We deliberately keep avahi out of the
  # NSS chain (nssmdns4/6 = false): nss-mdns probes `local. SOA` through the
  # system resolver, resolved drops that query, and the probe blocks 5s before
  # every .local lookup. Going straight through libnss_resolve sidesteps it.
  config = lib.mkIf cfg.enable {
    services.avahi = {
      enable = true;
      nssmdns4 = false;
      nssmdns6 = false;
      publish = lib.mkIf cfg.publish {
        enable = true;
        addresses = true;
      };
    };

    services.resolved.settings.Resolve.MulticastDNS = "resolve";

    # NM-managed links default to mdns=-1 (off). Flip them to "resolve" so
    # resolved actually does mDNS on those interfaces.
    networking.networkmanager.connectionConfig = lib.mkIf config.networking.networkmanager.enable {
      "connection.mdns" = 1;
    };

    # Drop libnss_resolve from the NSS hosts chain. Its varlink interface
    # only surfaces one address family at a time for mDNS-resolved names, so
    # `ssh -6 tsugumi.local` / `ping -6 tsugumi.local` fail even though
    # resolved has the AAAA cached. libnss_dns talks to the same resolved
    # over the 127.0.0.53 stub and returns both A and AAAA reliably.
    system.nssDatabases.hosts = lib.mkForce [
      "mymachines"
      "files"
      "myhostname"
      "dns"
    ];
  };
}
