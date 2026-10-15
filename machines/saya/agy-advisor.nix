{ config, lib, pkgs, ... }:
# A second opinion from Gemini for the agents, through Google's Antigravity CLI (agy) on
# Baughn's subscription (Baughn, msg 1560337741357645888: "Let's try it with `agy -p`").
# agy runs as its own user, which holds the sign-in: the agents only reach it through the
# socket, so they never see the token. It may read the repositories below (ACLs, read-only)
# and the web. Sign in once as Baughn: `agy-advisor-login` (browser on saya).
let
  agent = import ./agent-user.nix;
  user = "agy-advisor";
  home = "/var/lib/${user}";
  # Group that may use the socket.
  clients = "agy-advisor-clients";
  agy = pkgs.antigravity-cli;
  # What agy may read; the first is the default working directory.
  dirs = [ "${agent.workdir}/nixos" ];
  script = pkgs.writers.writePython3Bin "second-opinion" { flakeIgnore = [ "E501" ]; }
    (builtins.readFile ./agy-advisor.py);
  login = pkgs.writeShellScriptBin "agy-advisor-login" ''
    exec sudo -u ${user} env HOME=${home} ${lib.getExe agy} "$@"
  '';
in
{
  users.users.${user} = {
    isSystemUser = true;
    group = user;
    inherit home;
    createHome = true;
    homeMode = "700";
  };
  users.groups.${user} = { };
  users.groups.${clients}.members = [ agent.user ];

  environment.systemPackages = [ login ];

  # Read-only access to the repositories: traverse the parents, read the tree. The default
  # ACL covers files created later (if their mode has group read).
  systemd.tmpfiles.rules = lib.concatMap (dir: [
    "a+ ${dirOf dir} - - - - group:${user}:--x"
    "A+ ${dir} - - - - group:${user}:r-X,default:group:${user}:r-X"
  ]) dirs;

  systemd.sockets.agy-advisor = {
    description = "Second opinions from Gemini (agy) for the agents";
    wantedBy = [ "sockets.target" ];
    socketConfig = {
      ListenStream = "/run/agy-advisor/socket";
      SocketGroup = clients;
      SocketMode = "0660";
      Accept = true;
      MaxConnections = 3;
    };
  };

  systemd.services."agy-advisor@" = {
    description = "A second opinion from Gemini (agy)";
    environment = {
      AGY = lib.getExe agy;
      AGY_DIRS = lib.concatStringsSep ":" dirs;
      # The agents' shell commands time out after 10 minutes.
      AGY_TIMEOUT = "540";
      HOME = home;
    };
    path = [ pkgs.git pkgs.ripgrep ];
    serviceConfig = {
      ExecStart = "${lib.getExe script} serve";
      StandardInput = "socket";
      StandardOutput = "socket";
      StandardError = "journal";
      User = user;
      Group = user;
      RuntimeMaxSec = "11min";
      # Not too restrictive (Baughn): it reads the repositories and the web. It writes only its home.
      ProtectSystem = "strict";
      ProtectHome = "read-only";
      ReadWritePaths = [ home ];
      PrivateTmp = true;
      NoNewPrivileges = true;
    };
  };

  me.agentChannel.instances.saya = {
    path = [ script ];
    allow = [ "Bash(second-opinion *)" ];
  };
}
