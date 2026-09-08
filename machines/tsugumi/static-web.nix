{ pkgs, ... }:

let
  roots = {
    "brage.info" = "/srv/svein";
    "madoka.brage.info" = "/srv/minecraft";
    "ar-innna.brage.info" = "/srv/aquagon";
  };
  configFile = pkgs.writeText "caddy-static.json" (builtins.toJSON {
    admin.disabled = true;
    apps.http.servers.static = {
      listen = [ "unix//run/caddy-static/http.sock|0660" ];
      automatic_https.disable = true;
      routes = (pkgs.lib.mapAttrsToList (host: root: {
        match = [{ host = [ host ]; }];
        handle = [{ handler = "file_server"; inherit root; browse = { }; }];
        terminal = true;
      }) roots) ++ [{
        handle = [{ handler = "static_response"; status_code = 404; }];
      }];
    };
  });
in
{
  # User-controlled symlinks must never be followed by the TLS/key-owning
  # reverse proxy. This process has no credentials and cannot read its state.
  users.users.caddy-static = {
    isSystemUser = true;
    group = "caddy-static";
  };
  users.groups.caddy-static = { };
  users.users.caddy.extraGroups = [ "caddy-static" ];

  systemd.services.caddy-static = {
    description = "Unprivileged static websites";
    wantedBy = [ "multi-user.target" ];
    unitConfig.RequiresMountsFor = builtins.attrValues roots;
    serviceConfig = {
      ExecStart = "${pkgs.caddy}/bin/caddy run --config ${configFile}";
      User = "caddy-static";
      Group = "caddy-static";
      RuntimeDirectory = "caddy-static";
      RuntimeDirectoryMode = "0750";
      Environment = [ "XDG_CONFIG_HOME=/run/caddy-static" "XDG_DATA_HOME=/run/caddy-static" ];
      Restart = "on-failure";
      NoNewPrivileges = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      PrivateTmp = true;
      PrivateDevices = true;
      ProtectProc = "invisible";
      InaccessiblePaths = [ "-/var/lib/caddy" "-/run/agenix" "-/run/caddy" ];
      RestrictAddressFamilies = [ "AF_UNIX" ];
      CapabilityBoundingSet = "";
      UMask = "0077";
    };
  };
}
