{ config, ... }:
# Garibaldi (tsugumi-sec, on tsugumi) talks with Kim's agents in #garibaldi on Kim's IRC server
# (Baughn, msgs 1576045252958101748, 1576046647262183506). That server, 10.42.0.2 (escher), is a
# peer of saya's wg0 hub only, so saya relays the one port: tsugumi connects to 10.171.0.6:6667 over
# wg1, whose only peer is tsugumi, and the connection goes on to 10.42.0.2:6667. No routing or NAT;
# Kim's side sees saya (10.42.0.1). On tsugumi an nft rule lets the agent reach this port only.
let
  listen = "10.171.0.6:6667";
  target = "10.42.0.2:6667";
in
{
  systemd.sockets.irc-relay = {
    description = "Relay to Kim's IRC server, for tsugumi over wg1";
    wantedBy = [ "sockets.target" ];
    socketConfig = {
      ListenStream = listen;
      FreeBind = true; # wg1 may come up after the socket
      MaxConnections = 8;
      # Belt and braces with wg1's peer list: only tsugumi.
      IPAddressDeny = "any";
      IPAddressAllow = [ "10.171.0.1/32" ];
    };
  };

  systemd.services.irc-relay = {
    description = "Relay to Kim's IRC server (${target})";
    serviceConfig = {
      ExecStart = "${config.systemd.package}/lib/systemd/systemd-socket-proxyd ${target}";
      DynamicUser = true;
      PrivateTmp = true;
      PrivateDevices = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      NoNewPrivileges = true;
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
      IPAddressDeny = "any";
      IPAddressAllow = [ "10.171.0.1/32" "10.42.0.2/32" ];
    };
  };

  networking.firewall.interfaces.wg1.allowedTCPPorts = [ 6667 ];
}
