{ config, lib, pkgs, ... }:
# Posting to the agent channel without an agent: `discord-notify` sends through
# the watchdog webhook, so the agents see its messages as context, never as
# triggers. Only members of the discord-notify group can read the webhook;
# give a service access with `SupplementaryGroups = [ "discord-notify" ]`.
# Never add an agent's user to it.
let
  cfg = config.me.discordNotify;
  roster = import ../lib/agent-roster.nix;
  package = pkgs.writeScriptBin "discord-notify" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings
      [ "@webhook@" "@owner@" ]
      [ config.age.secrets.discord-notify-webhook.path (toString roster.humans.baughn.discordId) ]
      (builtins.readFile ./discord-notify.py)
  );
in
{
  options.me.discordNotify = {
    enable = lib.mkEnableOption "the discord-notify command for system services";
    package = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      default = package;
      description = "discord-notify, for use in services' ExecStart.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.groups.discord-notify = { };
    # The same webhook as minecraft-watch and agent-ship, readable by the group.
    age.secrets.discord-notify-webhook = {
      file = ../secrets/minecraft-watch-webhook.age;
      group = "discord-notify";
      mode = "0440";
    };
    environment.systemPackages = [ package ];
  };
}
