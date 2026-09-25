{ config, lib, pkgs, ... }:
# Claude Code agents as members of a Discord channel: one agent-bridge unit per
# identity, running as that identity's Unix user. See docs/agent-channel-design.md.
let
  cfg = config.me.agentChannel;
  roster = import ../lib/agent-roster.nix;
  bridge = pkgs.callPackage ../tools/agent-bridge { };
  toml = pkgs.formats.toml { };
  roles = lib.types.enum [ "owner" "admin" "agent" ];

  rosterToml = lib.filterAttrs (_: v: v != null) {
    guild_id = roster.guildId;
    admin_role_id = roster.adminRoleId;
    watchdog_webhook_id = roster.watchdogWebhookId;
    agents_role_id = roster.agentsRoleId or null;
    humans = lib.mapAttrs (_: h: { discord_id = h.discordId; inherit (h) role; }) roster.humans;
    agents = lib.mapAttrs (_: a: { discord_id = a.discordId; inherit (a) description; }) roster.agents;
  };

  instanceModule = { name, ... }: {
    options = {
      user = lib.mkOption { type = lib.types.str; description = "Unix user the bridge and agent run as."; };
      workdir = lib.mkOption {
        type = lib.types.str;
        description = "The agent's working directory (never $HOME): CLAUDE.md, tools/, notes/.";
      };
      channel = lib.mkOption {
        type = lib.types.enum [ "main" "test" ];
        default = "test";
        description = "Which roster channel this identity lives in.";
      };
      tokenFile = lib.mkOption { type = lib.types.str; description = "The bot's Discord token (read by root via LoadCredential)."; };
      claudeTokenFile = lib.mkOption {
        type = lib.types.str;
        default = cfg.claudeTokenFile;
        defaultText = "config.me.agentChannel.claudeTokenFile";
        description = "A `claude setup-token` OAuth token.";
      };
      triggers = lib.mkOption { type = lib.types.listOf roles; default = [ "owner" "admin" "agent" ]; };
      approvers = lib.mkOption { type = lib.types.listOf roles; default = [ "owner" "admin" ]; };
      ownerOnly = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Only the owner may trigger, approve or answer. Requires triggers = approvers = [ \"owner\" ].";
      };
      permissionMode = lib.mkOption { type = lib.types.enum [ "default" "auto" "acceptEdits" "plan" "dontAsk" ]; default = "default"; };
      allow = lib.mkOption { type = lib.types.listOf lib.types.str; default = [ ]; };
      ask = lib.mkOption { type = lib.types.listOf lib.types.str; default = [ ]; };
      deny = lib.mkOption { type = lib.types.listOf lib.types.str; default = [ ]; };
      promptFile = lib.mkOption { type = lib.types.nullOr lib.types.path; default = null; description = "Identity prompt, appended to the base prompt."; };
      model = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
      approvalTimeout = lib.mkOption { type = lib.types.int; default = 900; };
      limits = lib.mkOption { type = lib.types.attrsOf lib.types.int; default = { }; };
      extraDirs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = ''
          Directories besides the workdir that Claude Code treats as part of the
          project: read-only commands and reads there run without asking.
        '';
      };
      rcon = {
        root = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Enables the rcon tool for worlds in <root>/<world> (port and password from server.properties).";
        };
        readOnly = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [
            "list" "tps" "forge tps" "neoforge tps" "spark tps" "spark health"
            "forge entity list" "neoforge entity list" "forge dimensions" "neoforge dimensions"
            "whitelist list" "banlist"
            # Flare and Erisia's live inspector, as used by minecraft-tick-debug.
            "flare tps" "flare health" "flare sampler info" "help flare"
            "erisia-inspect status" "erisia-inspect census" "erisia-inspect chunk"
            "erisia-inspect watch status"
          ];
          description = "Console commands (by leading words) that run without an approver.";
        };
        ask = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [
            "stop" "save-off" "op" "deop" "ban" "ban-ip" "pardon" "pardon-ip" "kick"
            "whitelist" "kill" "gamerule" "difficulty" "defaultgamemode" "setworldspawn"
          ];
          description = ''
            Console commands (by leading words) that always go to an approver, even
            in auto mode. Read-only entries win (`whitelist list` stays read-only).
            In auto mode, everything else is left to the classifier.
          '';
        };
      };
      skills = lib.mkOption {
        type = lib.types.attrsOf lib.types.path;
        default = { };
        description = ''
          Claude Code skills, by name, linked read-only into <workdir>/.claude/skills
          and pre-approved for the Skill tool.
        '';
      };
      path = lib.mkOption {
        type = lib.types.listOf (lib.types.either lib.types.package lib.types.str);
        default = [ ];
        description = "Extra tools for the agent, on top of the system profile.";
      };
      serviceConfig = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        default = { };
        description = "OS-level restrictions (NoNewPrivileges, ProtectHome, ...).";
      };
      fake = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Test only: start without Discord or Claude, and idle.";
      };
    };
  };

  configFile = name: i: toml.generate "agent-bridge-${name}.toml" (lib.filterAttrs (_: v: v != null) {
    id = name;
    inherit (i) workdir triggers approvers allow ask deny model fake limits;
    channel_id = roster.channels.${i.channel};
    owner_only = i.ownerOnly;
    permission_mode = i.permissionMode;
    prompt_file = if i.promptFile == null then null else "${i.promptFile}";
    cli_path = lib.getExe pkgs.claude-code;
    approval_timeout = i.approvalTimeout;
    extra_dirs = i.extraDirs;
    rcon_root = i.rcon.root;
    rcon_read_only = i.rcon.readOnly;
    rcon_ask = i.rcon.ask;
    skills = lib.attrNames i.skills;
    roster = rosterToml;
  });
in
{
  options.me.agentChannel = {
    claudeTokenFile = lib.mkOption {
      type = lib.types.str;
      description = "Shared `claude setup-token` token for every identity on this machine.";
    };
    instances = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule instanceModule);
      default = { };
      description = "Agent identities running on this machine, keyed by roster agent name.";
    };
  };

  config = lib.mkIf (cfg.instances != { }) {
    assertions = lib.concatLists (lib.mapAttrsToList (name: i: [
      {
        assertion = roster.agents ? ${name};
        message = "agent-channel: ${name} is not in lib/agent-roster.nix";
      }
      {
        assertion = !i.ownerOnly || (i.triggers == [ "owner" ] && i.approvers == [ "owner" ]);
        message = "agent-channel: ${name} is ownerOnly, so triggers and approvers must be [ \"owner\" ]";
      }
      {
        assertion = !(lib.elem "agent" i.approvers);
        message = "agent-channel: agents can never approve (${name})";
      }
      {
        assertion = roster.channels.${i.channel} != null;
        message = "agent-channel: the roster has no ${i.channel} channel (${name})";
      }
    ]) cfg.instances);

    systemd.services = lib.mapAttrs' (name: i: lib.nameValuePair "agent-bridge-${name}" {
      description = "Discord agent bridge for ${name}";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      # The agent gets the system's tools, as in an interactive shell.
      path = [ "/run/current-system/sw" ] ++ i.path;
      environment = {
        AGENT_BRIDGE_CONFIG = "${configFile name i}";
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        DISABLE_AUTOUPDATER = "1";
      };
      serviceConfig = {
        ExecStart = "${lib.getExe bridge} run";
        User = i.user;
        WorkingDirectory = i.workdir;
        StateDirectory = "agent-bridge/${name}";
        StateDirectoryMode = "0700";
        RuntimeDirectory = "agent-bridge-${name}";
        RuntimeDirectoryMode = "0700";
        LoadCredential = [
          "discord-token:${i.tokenFile}"
          "claude-token:${i.claudeTokenFile}"
        ];
        UMask = "0077";
        Restart = "on-failure";
        RestartSec = "30s";
      } // i.serviceConfig;
    }) cfg.instances;

    systemd.tmpfiles.rules = lib.concatLists (lib.mapAttrsToList (name: i:
      [ "d ${i.workdir} 0700 ${i.user} - -" ]
      ++ lib.optionals (i.skills != { }) [
        "d ${i.workdir}/.claude 0700 ${i.user} - -"
        "d ${i.workdir}/.claude/skills 0700 ${i.user} - -"
      ]
      ++ lib.mapAttrsToList (skill: src: "L+ ${i.workdir}/.claude/skills/${skill} - - - - ${src}") i.skills
    ) cfg.instances);
  };
}
