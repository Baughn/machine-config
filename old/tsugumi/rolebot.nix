# Simple systemd unit for the 'rolebot' service
{ pkgs, ... }:
{
  systemd.services.rolebot = {
    description = "Rolebot";
    after = [ "network.target" ];
    wantedBy = [ "multi-user.target" ];
    path = with pkgs; [ cargo pkg-config openssl gcc ];
    serviceConfig = {
      Type = "simple";
      Restart = "on-failure";
      RestartSec = "5";
      User = "svein";
      Group = "users";
      WorkingDirectory = "/home/svein/dev/rolebot";
    };
    script = ''
      export RUST_LOG=info
      cargo run
    '';
  };
}
