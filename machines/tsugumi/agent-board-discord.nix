{ config, lib, pkgs, ... }:

# Copies the agent channel and its threads into the agent board's archive, so the agents
# can search the whole history (Baughn, msg 1555569477968724201). It posts to the board as
# the `discord` identity; only that identity may write the archive.
#
# The bot is a dedicated read-only one: View Channel and Read Message History on the agent
# channel (permissions integer 66560), plus the Message Content intent in the developer
# portal. Its token is secrets/agent-board-discord.age; until Baughn has created that file
# (agenix -e), this module does nothing.
let
  tokenFile = config.me.agentBoard.discordTokenFile;
  roster = import ../../lib/agent-roster.nix;
  agentIds = map (agent: agent.discordId) (builtins.attrValues roster.agents);
  configFile = pkgs.writeText "agent-board-discord.json" (builtins.toJSON {
    socket = "/run/agent-board/api.sock";
    guild = roster.guildId;
    channels = [ roster.channels.main ];
    agents = agentIds;
    # Roster names, so archive authors match the board's agent ids.
    names = lib.mapAttrs' (name: who: lib.nameValuePair who.discordId name)
      (roster.agents // roster.humans);
    interval = 30;
  });
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
