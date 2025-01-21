{ config, lib, pkgs, ... }:

let
  BOT_DIR = "/home/svein/dev/irc-tool/";

  # The bot's a simple rust app.
  bot = {
    description = "irc-tool";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    path = with pkgs; [ stdenv.cc ];
    serviceConfig = {
      User = "svein";
      WorkingDirectory = BOT_DIR;
      Type = "simple";
      Restart = "always";
      RestartSec = 10;
      ExecStart = "${pkgs.cargo}/bin/cargo run --release";
    };
  };
in
{
  systemd.services.irc-tool = bot;
}
