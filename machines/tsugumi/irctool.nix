{ config, lib, pkgs, ... }:

let
  irc-tool = pkgs.callPackage ../../tools/irc-tool { };
in
{
  systemd.services.irc-tool = {
    description = "irc-tool";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    serviceConfig = {
      User = "svein";
      Type = "simple";
      Restart = "always";
      RestartSec = 10;
      ExecStart = "${irc-tool}/bin/irc-tool ${config.age.secrets."irc-tool.env".path}";
    };
  };
}
