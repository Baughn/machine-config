let
  svein = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDFqQOHIaerfzhi0pQHZ/U1ES2yvql9NY46A01TjmgAl" # Tsugumi
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGs877ZtpIoKuc+Jn+GDISMBWxxGyZNdubdnqX2b6TV0" # Saya
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICeiqQnylmCVUzTNNcYRWKp/38dB5i3aGBs7ZB11MjkS" # Kaho
  ];
  users = svein;

  saya = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICc1Ua56Z0vcWw0ceHzVKjUW+S/zASlAzjLHCUGHx3QF";
  tsugumi = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBQ/0hKkb/+12T9ZzQ0lvu13JEL0RZJMxZ27WaQw9+3K";
  v4 = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICn/KDuz5Ie7wJx/s+8hGYur/vMuYoyv6ZkbA+y+cONa";
  systems = [ saya tsugumi v4 ];

  all = users ++ systems;
  host = h: [ h ] ++ svein;
in
{
  # Web stuff
  "caddy.env.age".publicKeys = host tsugumi;
  # Authelia
  "authelia-storage-key.age".publicKeys = host tsugumi;
  "authelia-jwt-key.age".publicKeys = host tsugumi;

  # Backup
  "restic.pw.age".publicKeys = all;

  # Rolebot
  "rolebot-config.json.age".publicKeys = host tsugumi;

  # IRC Tool
  "irc-tool.env.age".publicKeys = host tsugumi;

  # Webhook for machine pushes
  "erisia-webhook.url.age".publicKeys = all;

  # Monitoring
  "grafana-admin-password.age".publicKeys = host tsugumi;

  # WireGuard private keys
  "wireguard-saya.age".publicKeys = host saya;
  "wireguard-tsugumi.age".publicKeys = host tsugumi;

  # Redis
  "redis-password.age".publicKeys = host tsugumi;
  "redis-nixcheck-password.age".publicKeys = all;

  # Magic reboot - emergency reboot via UDP packet
  "magic-reboot.key.age".publicKeys = all;
}
