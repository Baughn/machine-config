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
        firefox = {
          executable = "${pkgs.firefox}/bin/firefox";
          profile = "${pkgs.firejail}/etc/firejail/firefox.profile";
        };
        discord = {
          executable = "${pkgs.discord}/bin/discord";
          profile = "${pkgs.firejail}/etc/firejail/discord.profile";
        };
      };
    };
  };
}
