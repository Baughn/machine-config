{ lib, ... }:

{
  services.udev.extraRules = ''
    SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x1557", NAME="lan"
    SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x159b", ATTR{phys_port_name}=="p0", NAME="p0_unused"
    SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x8086", ATTRS{device}=="0x159b", ATTR{phys_port_name}=="p1", NAME="lan"
    SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x10ec", ATTRS{device}=="0x8125", NAME="rj45_unused"
    SUBSYSTEM=="net", ACTION=="add", ATTRS{vendor}=="0x14c3", ATTRS{device}=="0x0616", NAME="wifi_unused"
  '';

  networking = {
    hostId = "deafbeef";
    useDHCP = false;
    interfaces.lan = {
      useDHCP = true;
      tempAddress = "disabled";
      ipv4.routes = [{
        address = "192.168.0.0";
        prefixLength = 24;
        options.mtu = "9000";
      }];
      ipv6.routes = [
        {
          address = "2a02:8086:d05:6780::";
          prefixLength = 64;
          options.mtu = "9000";
        }
        {
          address = "fe80::";
          prefixLength = 64;
          options.mtu = "9000";
        }
      ];
    };
    networkmanager.enable = false;
    firewall.allowPing = true;
    firewall.allowedUDPPorts = [
      34197
    ];
    # mDNS (5353) and LLMNR (5355) only from the LAN: lan also carries the public IPv6
    # prefix, so a plain open port exposed resolved to the internet (Garibaldi's review,
    # 2026-10-10; Baughn asked).
    firewall.extraCommands = lib.concatMapStrings (port: ''
      iptables -A nixos-fw -p udp --dport ${toString port} -s 192.168.0.0/24 -j nixos-fw-accept
      ip6tables -A nixos-fw -p udp --dport ${toString port} -s fe80::/10 -j nixos-fw-accept
    '') [ 5353 5355 ];
  };
}
