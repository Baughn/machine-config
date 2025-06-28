{ config, lib, pkgs, ... }:

let
  BOT_DIR = "/home/svein/AI/image-generation/sd-bot-2";
in
{
  systemd.services.sd-bot = {
    description = "sd-bot";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    path = [ pkgs.nix pkgs.cached-nix-shell ];
    serviceConfig = {
      User = "svein";
      WorkingDirectory = BOT_DIR;
      Type = "simple";
      Restart = "always";
      RestartSec = 10;
      Environment = "NIX_PATH=nixpkgs=/etc/nixpkgs";
      ExecStart = "${BOT_DIR}/start.sh";
    };
  };
}
