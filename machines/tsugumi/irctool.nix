{ config, lib, pkgs, ... }:

let
  irc-tool = pkgs.rustPlatform.buildRustPackage {
    pname = "irc-tool";
    version = "0.1.0";
    src = ../../tools/irc-tool;
    cargoLock.lockFile = ../../tools/irc-tool/Cargo.lock;
  };
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
