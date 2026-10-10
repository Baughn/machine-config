{ lib, ... }:

{
  services.resolved = {
    enable = true;
    settings.Resolve = {
      # Keys here are literal resolved.conf INI keys — case matters.
      # (Lowercase dnssec/domains were silently ignored for months.)
      DNSSEC = "allow-downgrade";
      Domains = [ "~." ];
      DNS = [
        "8.8.8.8#dns.google"
        "8.8.4.4#dns.google"
        "2001:4860:4860::8888#dns.google"
        "2001:4860:4860::8844#dns.google"
      ];
      FallbackDNS = [
        "1.1.1.1#cloudflare-dns.com"
        "1.0.0.1#cloudflare-dns.com"
        "2606:4700:4700::1111#cloudflare-dns.com"
        "2606:4700:4700::1001#cloudflare-dns.com"
      ];
      DNSOverTLS = "yes";
    };
  };

  # resolved (LLMNR, mDNS) and avahi listen on UDP 5353/5355. The LAN interfaces also carry
  # public IPv6, so a plain open port reaches them from the internet (Garibaldi's review,
  # 2026-10-10). Accept local name resolution only from the home LAN and link-local.
  networking.firewall.extraCommands = lib.concatMapStrings (port: ''
    iptables -A nixos-fw -p udp --dport ${toString port} -s 192.168.0.0/16 -j nixos-fw-accept
    ip6tables -A nixos-fw -p udp --dport ${toString port} -s fe80::/10 -j nixos-fw-accept
  '') [ 5353 5355 ];
}
