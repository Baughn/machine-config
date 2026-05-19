{ ... }:

{
  age.secrets = {
    "wireguard-tsugumi".file = ../../secrets/wireguard-tsugumi.age;
    "caddy.env".file = ../../secrets/caddy.env.age;

    "authelia-storage-key" = {
      file = ../../secrets/authelia-storage-key.age;
      owner = "authelia-main";
    };
    "authelia-jwt-key" = {
      file = ../../secrets/authelia-jwt-key.age;
      owner = "authelia-main";
    };

    "rolebot-config.json" = {
      file = ../../secrets/rolebot-config.json.age;
      owner = "svein";
      mode = "0400";
    };
    "irc-tool.env" = {
      file = ../../secrets/irc-tool.env.age;
      owner = "svein";
      mode = "0400";
    };

    "grafana-admin-password" = {
      file = ../../secrets/grafana-admin-password.age;
      owner = "grafana";
      mode = "0400";
    };

    "redis-password" = {
      file = ../../secrets/redis-password.age;
      owner = "redis-default";
      group = "wheel";
      mode = "0440";
    };
    "redis-nixcheck-password" = {
      file = ../../secrets/redis-nixcheck-password.age;
      owner = "svein";
      mode = "0400";
    };

    "rendezvous.key".file = ../../secrets/rendezvous.key.age;
    "claude-api.key".file = ../../secrets/claude-api.key.age;
    "anidb-user".file = ../../secrets/anidb-user.age;
    "anidb-password".file = ../../secrets/anidb-password.age;

    "cloudflare-dyndns-token".file = ../../secrets/cloudflare-dyndns-token.age;
  };
}
