{ config, lib, pkgs, ... }:
# A second opinion from Gemini for the agents, over the Gemini API with a $20 monthly budget
# (Baughn, msg 1560345194488926431; it replaces archive/agy-advisor). The service runs as its
# own user, which holds the API key: the agents only reach it through the socket. Gemini gets
# Google Search, URL context and read-only file tools for the repositories below, no shell.
# The budget is kept twice: the service's ledger refuses calls past it, and the AI Studio
# project has a spend cap of the same size.
let
  agent = import ./agent-user.nix;
  user = "second-opinion";
  state = "/var/lib/${user}";
  clients = "second-opinion-clients";
  # Encrypted for [ svein saya ]; holds only the AI Studio key. Off until it exists.
  keyFile = ../../secrets/gemini-api-key.age;
  enabled = builtins.pathExists keyFile;
  # What Gemini may read; the first is the default working directory.
  dirs = [ "${agent.workdir}/nixos" ];
  script = pkgs.writers.writePython3Bin "second-opinion"
    { libraries = [ pkgs.python3Packages.google-genai ]; flakeIgnore = [ "E501" ]; }
    (builtins.readFile ./second-opinion.py);
in
lib.mkIf enabled {
  age.secrets.gemini-api-key = {
    file = keyFile;
    owner = user;
  };

  users.users.${user} = {
    isSystemUser = true;
    group = user;
    home = state;
  };
  users.groups.${user} = { };
  users.groups.${clients}.members = [ agent.user ];

  # Read access to the repositories; the default ACL covers files created later (if their
  # mode has group read). The unit binds them into an empty /home, so the parents (the agent's
  # 0700 workdir, which tmpfiles keeps at 0700 and so masks any ACL) needn't be traversable.
  systemd.tmpfiles.rules = map (dir: "A+ ${dir} - - - - group:${user}:r-X,default:group:${user}:r-X") dirs;

  systemd.sockets.second-opinion = {
    description = "Second opinions from Gemini for the agents";
    wantedBy = [ "sockets.target" ];
    socketConfig = {
      ListenStream = "/run/second-opinion/socket";
      SocketGroup = clients;
      SocketMode = "0660";
      Accept = true;
      MaxConnections = 2;
    };
  };

  systemd.services."second-opinion@" = {
    description = "A second opinion from Gemini";
    environment = {
      GEMINI_API_KEY_FILE = config.age.secrets.gemini-api-key.path;
      SO_MODEL = "gemini-4-argon";
      SO_DIRS = lib.concatStringsSep ":" dirs;
      SO_LEDGER = "${state}/ledger.json";
      SO_MONTHLY_USD = "20";
      SO_CALL_USD = "1";
      # The agents' shell commands time out after 10 minutes.
      SO_TIMEOUT = "540";
    };
    path = [ pkgs.ripgrep ];
    serviceConfig = {
      ExecStart = "${lib.getExe script} serve";
      StandardInput = "socket";
      StandardOutput = "socket";
      StandardError = "journal";
      User = user;
      Group = user;
      StateDirectory = user;
      StateDirectoryMode = "0700";
      RuntimeMaxSec = "11min";
      ProtectSystem = "strict";
      ProtectHome = "tmpfs";
      BindReadOnlyPaths = dirs;
      PrivateTmp = true;
      PrivateDevices = true;
      NoNewPrivileges = true;
      CapabilityBoundingSet = "";
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
      SystemCallFilter = [ "@system-service" ];
    };
  };

  me.agentChannel.instances.saya = {
    path = [ script ];
    allow = [ "Bash(second-opinion *)" ];
  };
}
