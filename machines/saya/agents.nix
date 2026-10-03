{ config, lib, pkgs, ... }:
# The saya agent: edits machine-config for Baughn and the other agents, and
# ships through agent-ship.nix. See "The saya identity" in
# docs/agent-channel-design.md.
let
  agent = import ./agent-user.nix;
  repo = "${agent.workdir}/nixos";
  # The agent board's token (machines/tsugumi/agent-board-saya.nix); no board until it exists.
  boardToken = ../../secrets/agent-board-saya-token.age;
  hasBoard = builtins.pathExists boardToken;
  # The client tester (agent-client.nix), once it's in the roster.
  client = lib.optional ((import ../../lib/agent-roster.nix).agents ? saya-client) "saya-client";

  # A clone of the public repo, made on first start. Failure (e.g. no network
  # yet) is logged and retried on the next start; the agent can clone it too.
  clone = pkgs.writeShellScript "saya-agent-clone" ''
    [ -d ${repo}/.jj ] && exit 0
    rm -rf ${repo}
    exec jj git clone --colocate https://github.com/Baughn/machine-config ${repo}
  '';

  # Its saya/* bookmarks as a bundle: data, so fetching it runs nothing of its.
  publish = pkgs.writeShellScriptBin "agent-publish" ''
    set -eu
    umask 022  # the unit's is 0077; Baughn fetches this as svein
    git -C ${repo} bundle create ${agent.outbox}/nixos.bundle.tmp --branches='saya/*'
    mv ${agent.outbox}/nixos.bundle.tmp ${agent.outbox}/nixos.bundle
    git -C ${repo} for-each-ref --format='%(refname:short) %(objectname:short) %(subject)' 'refs/heads/saya/'
  '';
in
{
  age.secrets = {
    agent-claude-token.file = ../../secrets/agent-claude-token.age;
    agent-saya-discord.file = ../../secrets/agent-saya-discord.age;
  } // lib.optionalAttrs hasBoard {
    agent-board-saya-token.file = boardToken;
  };

  users = agent.users;

  systemd.tmpfiles.rules = [ "d ${agent.outbox} 0755 ${agent.user} ${agent.user} -" ];

  me.agentChannel = {
    claudeTokenFile = config.age.secrets.agent-claude-token.path;

    instances.saya = {
      inherit (agent) user workdir;
      channel = "main";
      tokenFile = config.age.secrets.agent-saya-discord.path;
      promptFile = ./agents/saya.md;
      board = lib.mkIf hasBoard {
        url = "http://10.171.0.1:8740";
        tokenFile = config.age.secrets.agent-board-saya-token.path;
      };
      # Baughn and the other agents may ask it; only Baughn approves. Admins
      # reach it only through an agent, and nothing leaves saya without
      # Baughn's ✅ on the agent-ship request.
      triggers = [ "owner" "agent" ];
      approvers = [ "owner" ];
      # Agents can trigger it, so a reply loop between two agents stops after a few turns.
      limits.bot_streak = 8;
      ship = {
        unit = "agent-ship";
        inherit repo;
        logs = "/var/lib/agent-ship";
      };
      askAgents = [ "tsugumi-minecraft" "tsugumi-lab" ] ++ client;
      permissionMode = "auto";
      model = "claude-opus-5-5";
      path = [ pkgs.jujutsu pkgs.git publish ];
      environment = {
        JJ_USER = "Svein Ove Aas";
        JJ_EMAIL = "sveina@gmail.com";
      };
      allow = [
        "Read"
        "Grep"
        "Glob"
        "WebSearch"
        "Edit(/${agent.workdir}/**)"
        "Write(/${agent.workdir}/**)"
        "Bash(jj *)"
        "Bash(nix build *)"
        "Bash(nix eval *)"
        "Bash(nix flake check *)"
        "Bash(nix flake show *)"
        "Bash(nix log *)"
        "Bash(rg *)"
        "Bash(agent-publish)"
      ];
      # jj can run arbitrary programs; the OS user is the real boundary.
      deny = [ "Bash(sudo *)" "Bash(deploy *)" "Bash(nixos-rebuild *)" "Bash(jj util exec*)" "Bash(jj fix*)" ];
      serviceConfig = agent.serviceConfig // {
        ExecStartPre = [ "-${clone}" ];
        # The first start clones the repo (over 100 MB).
        TimeoutStartSec = "15min";
      };
    };
  };
}
