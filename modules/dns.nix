{ ... }:

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
}
