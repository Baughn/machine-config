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
    
    # Backup
    "restic.pw" = {
      file = ./restic.pw.age;
      hosts = ["saya"];
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
