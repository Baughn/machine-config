# Simple systemd unit for the 'rolebot' service
{ config, pkgs, ... }:
let
  rolebot = pkgs.rustPlatform.buildRustPackage {
    pname = "rolebot";
    version = "0.1.0";
    src = ../../tools/rolebot;
    cargoLock.lockFile = ../../tools/rolebot/Cargo.lock;
  };
in
{
  systemd.services.rolebot = {
    description = "Rolebot";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      Restart = "on-failure";
      RestartSec = "5";
      User = "svein";
      Group = "users";
      ExecStart = "${rolebot}/bin/rolebot ${config.age.secrets."rolebot-config.json".path}";
    };
    environment = {
      RUST_LOG = "info";
    };
  };
}
