{ config, lib, pkgs, ... }:
# agent-ship@<commit>: pushes and deploys a saya-agent commit after Baughn's ✅.
# See agent-ship.py and "The saya identity" in docs/agent-channel-design.md.
let
  roster = import ../../lib/agent-roster.nix;
  agent = import ./agent-user.nix;
  ship = pkgs.writeScriptBin "agent-ship" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings
      [ "@git@" "@jj@" "@origin@" "@bundle@" "@userRepo@" "@owner@" "@channel@" ]
      [ "${pkgs.git}/bin/git" "${pkgs.jujutsu}/bin/jj" "git@github.com:Baughn/machine-config"
        "${agent.outbox}/nixos.bundle" "/home/svein/nixos" roster.humans.baughn.discordId
        roster.channels.main ]
      (builtins.readFile ./agent-ship.py)
  );
in
{
  age.secrets.minecraft-watch-webhook.file = ../../secrets/minecraft-watch-webhook.age;

  systemd.services."agent-ship@" = {
    description = "Push and deploy agent commit %i after Baughn approves";
    # Deploying saya switches this very system; don't let that kill the run.
    restartIfChanged = false;
    stopIfChanged = false;
    # deploy needs nix, ssh and sudo, as in Baughn's shell.
    path = [ "/run/wrappers" "/run/current-system/sw" ];
    environment.SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    serviceConfig = {
      Type = "oneshot";
      User = "svein";
      ExecStart = "${ship}/bin/agent-ship %i";
      StateDirectory = "agent-ship";
      StateDirectoryMode = "0755"; # the agent reads <commit>.log
      LoadCredential = [
        "webhook:${config.age.secrets.minecraft-watch-webhook.path}"
        "discord-token:${config.age.secrets.agent-saya-discord.path}"
      ];
      TimeoutStartSec = "3h"; # up to an hour for the approval, then the deploy
    };
  };

  # The agent may start agent-ship@<commit>; the unit checks everything itself.
  security.polkit.extraConfig = ''
    polkit.addRule(function (action, subject) {
      if (action.id == "org.freedesktop.systemd1.manage-units" &&
          subject.user == "${agent.user}" &&
          /^agent-ship@[0-9a-f]{40}\.service$/.test(action.lookup("unit")) &&
          action.lookup("verb") == "start") {
        return polkit.Result.YES;
      }
    });
  '';
}
