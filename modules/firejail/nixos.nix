{ config, lib, pkgs, ... }:

let
  cfg = config.me.firejail;
in
{
  options.me.firejail = {
    enable = lib.mkEnableOption "Firejail application sandboxing";
  };

  config = lib.mkIf cfg.enable {
    # Requires apparmor
    security.apparmor.enable = true;

    programs.firejail = {
      enable = true;
      wrappedBinaries = {
        discord = {
          executable = "${pkgs.discord}/bin/discord";
          profile = "${pkgs.firejail}/etc/firejail/discord.profile";
        };
      };
    };
  };
}
