let
  # Host keys — each machine's /etc/ssh/ssh_host_ed25519_key.pub
  saya = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHkaJd61/WV8hrah8wsuuTVmTBM4JsU1UWJMQyABaHVY root@saya";
  tsugumi = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBQ/0hKkb/+12T9ZzQ0lvu13JEL0RZJMxZ27WaQw9+3K";
  v4   = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICn/KDuz5Ie7wJx/s+8hGYur/vMuYoyv6ZkbA+y+cONa";

  # User keys — for encrypting secrets during development
  svein = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGppkBITukYVejPl3BiRmCDSfdrItzM59XpwwK7W/mXH svein@saya";

  allKeys = [ svein saya tsugumi v4 ];
in
{
  "wireguard-saya.age".publicKeys = [ svein saya ];
  "wireguard-tsugumi.age".publicKeys = [ svein tsugumi ];
  "magic-reboot.key.age".publicKeys = allKeys;

  "restic-password.age".publicKeys = [ svein saya ];

  "caddy.env.age".publicKeys = [ svein tsugumi ];
  "authelia-storage-key.age".publicKeys = [ svein tsugumi ];
  "authelia-jwt-key.age".publicKeys = [ svein tsugumi ];
  "rolebot-config.json.age".publicKeys = [ svein tsugumi ];
  "irc-tool.env.age".publicKeys = [ svein tsugumi ];
  "grafana-admin-password.age".publicKeys = [ svein tsugumi ];
  "redis-password.age".publicKeys = [ svein tsugumi ];
  "redis-nixcheck-password.age".publicKeys = allKeys;
  "rendezvous.key.age".publicKeys = [ svein tsugumi ];
  "claude-api.key.age".publicKeys = [ svein tsugumi ];
  "anidb-user.age".publicKeys = [ svein tsugumi ];
  "anidb-password.age".publicKeys = [ svein tsugumi ];
}
