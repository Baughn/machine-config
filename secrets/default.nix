{
  config,
  lib,
  ...
}: let
  secrets = {
    # Web stuff
    "caddy.env" = {
      file = ./caddy.env.age;
      hosts = ["tsugumi"];
    };
    
    # Authelia
    "authelia-storage-key" = {
      file = ./authelia-storage-key.age;
      owner = "authelia-main";
      hosts = ["tsugumi"];
    };
    "authelia-jwt-key" = {
      file = ./authelia-jwt-key.age;
      owner = "authelia-main";
      hosts = ["tsugumi"];
    };

    # Backup
    "restic.pw" = {
      file = ./restic.pw.age;
      hosts = ["saya"];
      owner = "svein";
      mode = "0400";
    };
    
    # Rolebot
    "rolebot-config.json" = {
      file = ./rolebot-config.json.age;
      hosts = ["tsugumi"];
      owner = "svein";
      mode = "0400";
    };
    
    # IRC Tool
    "irc-tool.env" = {
      file = ./irc-tool.env.age;
      hosts = ["tsugumi"];
      owner = "svein";
      mode = "0400";
    };
  };
in {
  age.secrets = lib.filterAttrs
    (name: value: value != null)
    (lib.mapAttrs
      (name: value:
        if value ? hosts then
          if builtins.elem config.networking.hostName value.hosts then
            builtins.removeAttrs value ["hosts"]
          else
            null
        else
          value
      )
      secrets);
}
