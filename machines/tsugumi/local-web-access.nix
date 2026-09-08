{ pkgs, ... }:

let
  rules = pkgs.writeText "local-web-access.nft" ''
    destroy table inet local_web_access
    table inet local_web_access {
      chain output {
        type filter hook output priority -10; policy accept;
        meta l4proto tcp fib daddr type local tcp dport { 3000, 8123, 8124 } jump backends
      }
      chain backends {
        meta skuid { "root", "caddy" } return
        tcp dport 3000 meta skuid "silverbullet" return
        tcp dport { 8123, 8124 } meta skuid "minecraft" return
        reject with tcp reset
      }
    }
  '';
in
{
  # Backend owners already control their services. Unrelated local accounts
  # must go through Caddy's authentication/read-only policy, even over loopback.
  networking.firewall.extraCommands = ''
    ${pkgs.nftables}/bin/nft -f ${rules}
  '';
  # Deliberately retain this independent table during firewall stop/reload.
  # nft replaces it atomically; removing/flushing a live guard opens a bypass.
}
