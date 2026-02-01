# This file must be kept in sync with secrets.nix.
{ config
, lib
, ...
}:
let
  secrets = {
    # Web stuff
    "caddy.env" = {
      file = ./caddy.env.age;
      hosts = [ "tsugumi" ];
    };

    # Authelia
    "authelia-storage-key" = {
      file = ./authelia-storage-key.age;
      owner = "authelia-main";
      hosts = [ "tsugumi" ];
    };
    "authelia-jwt-key" = {
      file = ./authelia-jwt-key.age;
      owner = "authelia-main";
      hosts = [ "tsugumi" ];
    };

    # Backup
    "restic.pw" = {
      file = ./restic.pw.age;
      hosts = [ "saya" ];
      owner = "svein";
      mode = "0400";
    };

    # Rolebot
    "rolebot-config.json" = {
      file = ./rolebot-config.json.age;
      hosts = [ "tsugumi" ];
      owner = "svein";
      mode = "0400";
    };

    # IRC Tool
    "irc-tool.env" = {
      file = ./irc-tool.env.age;
      hosts = [ "tsugumi" ];
      owner = "svein";
      mode = "0400";
    };

    # Erisia webhook
    "erisia-webhook.url" = {
      file = ./erisia-webhook.url.age;
      hosts = [ "saya" ];
      owner = "svein";
      mode = "0400";
    };

    # Monitoring
    "grafana-admin-password" = {
      file = ./grafana-admin-password.age;
      hosts = [ "tsugumi" ];
      owner = "grafana";
      mode = "0400";
    };

    # Redis
    "redis-password" = {
      file = ./redis-password.age;
      hosts = [ "tsugumi" ];
      owner = "redis-default";
      group = "wheel";
      mode = "0440";
    };
    "redis-nixcheck-password" = {
      file = ./redis-nixcheck-password.age;
      hosts = [ "saya" "tsugumi" ];
      owner = "svein";
      mode = "0400";
    };
    # DessPlay rendezvous server
    "rendezvous.key" = {
      file = ./rendezvous.key.age;
      hosts = [ "v4" ];
    };
  };
in
{
  age.secrets = lib.filterAttrs
    (name: value: value != null)
    (lib.mapAttrs
      (name: value:
        if value ? hosts then
          if builtins.elem config.networking.hostName value.hosts then
            builtins.removeAttrs value [ "hosts" ]
          else
            null
        else
          value
      )
      secrets);
}
