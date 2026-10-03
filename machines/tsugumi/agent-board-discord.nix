{ config, lib, pkgs, ... }:

# Copies the agent channel and its threads into the agent board's archive, so the agents
# can search the whole history (Baughn, msg 1555569477968724201). It posts to the board as
# the `discord` identity; only that identity may write the archive.
#
# The bot is a dedicated read-only one: View Channel and Read Message History on the agent
# channel (permissions integer 66560), plus the Message Content intent in the developer
# portal. Its token is secrets/agent-board-discord.age; until Baughn has created that file
# (agenix -e), this module does nothing.
#
# Its application also answers `/board` (login links for the board's pages): the poller
# registers the command at startup, and Discord delivers it to the Interactions Endpoint URL
# https://agents.brage.info/discord/interactions. That needs the bot invited with the
# applications.commands scope as well, and me.agentBoard.discordPublicKey set.
let
  tokenFile = config.me.agentBoard.discordTokenFile;
  roster = import ../../lib/agent-roster.nix;
  agentIds = map (agent: agent.discordId) (builtins.attrValues roster.agents);
  configFile = pkgs.writeText "agent-board-discord.json" (builtins.toJSON ({
    socket = "/run/agent-board/api.sock";
    guild = roster.guildId;
    channels = [ roster.channels.main ];
    agents = agentIds;
    # Roster names, so archive authors match the board's agent ids.
    names = lib.mapAttrs' (name: who: lib.nameValuePair who.discordId name)
      (roster.agents // roster.humans);
    interval = 30;
  } // lib.optionalAttrs (config.me.agentBoard.discordPublicKey != null) {
    # Answered by the board over the interactions endpoint (agent-board.nix).
    commands = [{
      name = "board";
      type = 1;
      description = "Get a one-time login link for the agent board (agents.brage.info)";
    }];
  }));
in
{
  options.me.agentBoard.discordTokenFile = lib.mkOption {
    type = lib.types.path;
    default = ../../secrets/agent-board-discord.age;
    description = "The agenix file with the archive bot's token; the poller runs once it exists.";
  };

  config = lib.mkIf (builtins.pathExists tokenFile) {
    age.secrets.agent-board-discord.file = tokenFile;

    users.users.agent-board-discord = {
      isSystemUser = true;
      group = "agent-board-discord";
    };
    users.groups.agent-board-discord = { };
    me.agentBoard.users.agent-board-discord = "discord";
    # The archive bot application's public key, for /board (Baughn, msg 1555916312549662846).
    me.agentBoard.discordPublicKey = "987cea47a1d96144023538fb9959dd1b9505165517fb6d0555de6964f266c45a";

    systemd.services.agent-board-discord = {
      description = "Agent board: copy the Discord channel into the archive";
      after = [ "network-online.target" "agent-board-api.socket" ];
      wants = [ "network-online.target" ];
      requires = [ "agent-board-api.socket" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${pkgs.python3}/bin/python3 -I ${./agent-board-discord.py} ${configFile}";
        LoadCredential = [ "discord-token:${config.age.secrets.agent-board-discord.path}" ];
        User = "agent-board-discord";
        Group = "agent-board-discord";
        Restart = "always";
        RestartSec = 30;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictNamespaces = true;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        LockPersonality = true;
        SystemCallArchitectures = "native";
        CapabilityBoundingSet = "";
        MemoryMax = "256M";
      };
    };
  };
}
