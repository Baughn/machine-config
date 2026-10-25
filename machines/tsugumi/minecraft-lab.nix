{ config, lib, pkgs, ... }:
# Minecraft labs: writable ZFS clones of the worlds, and servers running on
# them, each lab for one agent (labs.main: tsugumi-lab, user mclab). Clones are
# made by the root helper (minecraft-storage lab …), which a lab's user reaches
# only through its own control socket. Each lab's servers and agent share a
# network namespace, linked to the internet by pasta and fenced off from the
# host and private addresses. See docs/agent-channel-design.md, "The lab identity".
#
# labs.main keeps the names it had when it was the only lab (mclab, minecraft-lab@,
# /run/minecraft-lab.sock, /srv/minecraft-lab, rpool/minecraft-lab); another lab
# NAME gets the same names with "-NAME" after the lab part (mclab-NAME,
# minecraft-lab-NAME@, /srv/minecraft-lab-NAME, ...). Baughn, msg 1563890697965404163:
# a second lab should be a simple config change.
let
  labs = config.me.minecraft.labs;
  storage = config.system.build.minecraft-storage;

  # The same command for every lab's user: the caller's uid picks its lab's socket.
  client = pkgs.writeScriptBin "minecraft-lab" ''
    #!${pkgs.python3}/bin/python3 -I
    """minecraft-lab list | clone WORLD|rpool/minecraft/WORLD@SNAPSHOT NAME | destroy NAME"""
    import json, os, pwd, socket, sys
    sockets = json.loads(${builtins.toJSON (builtins.toJSON (lib.mapAttrs' (_: lab: lib.nameValuePair lab.user lab.socket) labs))})
    try:
        path = sockets.get(pwd.getpwuid(os.getuid()).pw_name, ${builtins.toJSON labs.main.socket})
    except KeyError:
        path = ${builtins.toJSON labs.main.socket}
    connection = socket.socket(socket.AF_UNIX)
    connection.connect(path)
    connection.sendall(json.dumps(["lab", *sys.argv[1:]]).encode() + b"\n")
    connection.shutdown(socket.SHUT_WR)
    reply = b"".join(iter(lambda: connection.recv(65536), b"")).decode(errors="replace")
    body, _, last = reply.rstrip("\n").rpartition("\n")
    if not last.startswith("exit: "):
        body, last = reply, "exit: 1"
    print(body, file=sys.stdout if last == "exit: 0" else sys.stderr)
    sys.exit(int(last.removeprefix("exit: ")))
  '';

  # nscd resolves for most programs; this covers anything reading resolv.conf,
  # since resolved's stub (127.0.0.53) is unreachable from the namespace.
  resolvConf = pkgs.writeText "mclab-resolv.conf" ''
    nameserver 1.1.1.1
    nameserver 9.9.9.9
  '';

  # Output from the lab user's sockets (pasta carries every lab packet, so all of
  # them) may go to the internet only: not to this host, not to private ranges.
  # Replies on connections others opened (the login port forward) pass. A positive
  # match and jump: packets without a socket (NDP, IGMP, WireGuard's kernel socket)
  # match no skuid and must fall through to accept, not reach the drops.
  fence = lab: pkgs.writeText "${lab.slice}-fence.nft" ''
    table inet ${lab.fenceTable}
    delete table inet ${lab.fenceTable}
    table inet ${lab.fenceTable} {
      chain output {
        type filter hook output priority filter; policy accept;
        meta skuid "${lab.user}" jump lab
      }
      chain lab {
        ct direction reply accept
        meta nfproto ipv6 drop
        fib daddr type local drop
        ip daddr { ${lib.concatStringsSep ", " lab.fenced} } drop
      }
    }
  '';

  stop = lab: pkgs.writeShellScript "${lab.unit}-stop" ''
    [[ -n ''${MAINPID:-} ]] || exit 0
    exec "${lab.mounts}/$1/control.sh" stop -t 10
  '';

  # Lab names become part of unit names (minecraft-lab-NAME-pasta, ...); these
  # would collide with the main lab's own units.
  reserved = [ "control" "setup" "expire" "fence" "pasta" "netcheck" "status" "login" "stop" ];

  labOptions = { name, config, ... }:
    let suffix = if name == "main" then "" else "-${name}"; in {
      options = {
        user = lib.mkOption {
          type = lib.types.str;
          default = "mclab${suffix}";
          readOnly = true;
          description = "The lab's user: its agent runs as it, and owns its clones.";
        };
        agent = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          example = "tsugumi-lab";
          description = ''
            The agent-channel instance working in this lab: crash-analysis triggers
            from its servers go to it, netcheck restarts its bridge after pasta, and
            the lab's status card (posted as the lab user) runs only when it is set.
          '';
        };
        maxClones = lib.mkOption { type = lib.types.int; default = 3; };
        lifetime = lib.mkOption {
          type = lib.types.int;
          default = 5 * 86400;
          description = "Seconds until a clone expires; shorter than the SSD's 7-day snapshot retention.";
        };
        quota = lib.mkOption { type = lib.types.str; default = "200G"; description = "Quota on the lab's dataset."; };
        scrub = lib.mkOption {
          type = lib.types.attrsOf (lib.types.listOf lib.types.str);
          default = {
            "config/Discord-Integration.toml" = [ "botToken" ];
            # Incognito's Discord bridge, [General] token (a lab clone joined Discord with it, 10-21).
            "config/mc2discord.toml" = [ "token" ];
            "config/thump/services/irc.cfg" = [ "NickservPassword" "SASLPassword" "ServerPassword" ];
          };
          description = ''
            Outbound credentials blanked in every clone, by path relative to the world:
            the agent reads the clone, and lab servers reach the internet.
            rcon.password is re-randomized separately.
          '';
        };
        fenced = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ "0.0.0.0/8" "10.0.0.0/8" "100.64.0.0/10" "127.0.0.0/8" "169.254.0.0/16"
                      "172.16.0.0/12" "192.168.0.0/16" "224.0.0.0/3" ];
          description = "IPv4 ranges the lab can't reach (IPv6 is off entirely).";
        };
        loginPort = lib.mkOption ({
          type = lib.types.port;
          description = "Host 127.0.0.1 port forwarded to the lab's 25565: ssh -L 25565:localhost:<port> tsugumi.";
        } // lib.optionalAttrs (name == "main") { default = 25665; });
        remoteLogin = {
          address = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            example = "10.171.0.1:25666";
            description = "Also offer the login port here (wg0), for the listed clients only.";
          };
          allow = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = "Client addresses allowed to connect to remoteLogin.address.";
          };
        };
        outbox = {
          port = lib.mkOption {
            type = lib.types.nullOr lib.types.port;
            default = null;
            example = 8741;
            description = ''
              Serve the lab's outbox directory (builds the lab hands to remote testers)
              read-only over HTTP on this wg0 port, to the listed clients only.
            '';
          };
          allow = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ ];
            description = "Client addresses allowed to fetch from the outbox.";
          };
        };
        memoryMax = lib.mkOption {
          type = lib.types.str;
          default = "16G";
          description = "MemoryMax of the lab's slice, every lab server together.";
        };
        cpuQuota = lib.mkOption { type = lib.types.str; default = "800%"; };

        # Names derived from the lab's name, for the units here and the agent's config.
        unit = lib.mkOption { type = lib.types.str; readOnly = true; default = "minecraft-lab${suffix}"; description = "Unit name prefix: <unit>@CLONE is a lab server."; };
        slice = lib.mkOption { type = lib.types.str; readOnly = true; default = "mclab${suffix}"; description = "The lab servers' slice, without .slice."; };
        fenceTable = lib.mkOption { type = lib.types.str; readOnly = true; default = "mclab${suffix}-fence"; };
        netns = lib.mkOption { type = lib.types.str; readOnly = true; default = "/run/netns/mclab${suffix}"; };
        socket = lib.mkOption { type = lib.types.str; readOnly = true; default = "/run/minecraft-lab${suffix}.sock"; };
        dataset = lib.mkOption { type = lib.types.str; readOnly = true; default = "rpool/minecraft-lab${suffix}"; };
        mounts = lib.mkOption { type = lib.types.str; readOnly = true; default = "/srv/minecraft-lab${suffix}"; description = "Where the lab's clones are mounted, one directory each."; };
        gcroots = lib.mkOption { type = lib.types.str; readOnly = true; default = "/nix/var/nix/gcroots/minecraft-lab${suffix}"; };
        handover = lib.mkOption { type = lib.types.str; readOnly = true; default = "/srv/lab-handover${suffix}"; description = "Handover to production; the lab writes, tsugumi-minecraft reads."; };
        outboxDir = lib.mkOption { type = lib.types.str; readOnly = true; default = "/srv/lab-outbox${suffix}"; };
        serviceConfig = lib.mkOption {
          type = lib.types.attrsOf lib.types.anything;
          readOnly = true;
          description = "Sandbox shared by the lab's units (the agent bridge and its lab servers).";
          default = {
            NetworkNamespacePath = config.netns;
            # Hides /home/minecraft (others may read it) and every other home.
            ProtectHome = true;
            PrivateTmp = true;
            NoNewPrivileges = true;
            BindReadOnlyPaths = [ "${resolvConf}:/etc/resolv.conf" ];
          };
        };
      };
    };

  # One lab's users, directories, sockets, units and polkit rule.
  labConfig = name: lab: {
    users.users.${lab.user} = {
      isSystemUser = true;
      group = lab.user;
      home = "/var/lib/${lab.user}";
      createHome = true;
      homeMode = "700";
      description = "Minecraft lab agent";
    };
    users.groups.${lab.user} = { };

    # Handover from the lab to production (mods, configs, instructions): the
    # lab writes, tsugumi-minecraft reads. setgid keeps new entries in the
    # group; files copied with mode 0600 still need chmod g+r.
    # The lab is a member too, so it can chgrp what it makes there (cp keeps
    # the lab's group; the bridge's umask makes files 0600).
    users.groups.lab-handover.members = [ lab.user ];

    systemd.tmpfiles.rules = [
      "d ${lab.mounts} 0755 root root -"
      "d ${lab.handover} 2750 ${lab.user} lab-handover -"
    ] ++ lib.optional (lab.outbox.port != null)
      # The lab writes; caddy serves it to the testers, so it is world-readable.
      "d ${lab.outboxDir} 0755 ${lab.user} ${lab.user} -";

    # Caddy listens on every address (wg0 may come up after it); the firewall
    # opens the port on wg0 only, and the matcher admits only the testers.
    services.caddy.extraConfig = lib.mkIf (lab.outbox.port != null) ''
      http://:${toString lab.outbox.port} {
        @denied not remote_ip ${lib.concatStringsSep " " lab.outbox.allow}
        abort @denied
        root * ${lab.outboxDir}
        file_server browse
      }
    '';

    systemd.sockets."${lab.unit}-control" = {
      description = "Minecraft lab helper${lib.optionalString (name != "main") " (lab ${name})"}";
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = lab.socket;
        SocketUser = "root";
        SocketGroup = lab.user;
        SocketMode = "0660";
        Accept = true;
        MaxConnections = 4;
      };
    };

    # The login port for remote clients: saya's client-tester agent joins lab servers here.
    # A socket unit's IP access list applies to its listening socket (so to who may connect),
    # not to the proxy's own connection to the login port. The proxy runs outside the lab
    # and its fence; it only ever reaches pasta's host-side listener.
    systemd.sockets."${lab.unit}-login-remote" = lib.mkIf (lab.remoteLogin.address != null) {
      description = "Remote clients' way to the lab's login port";
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = lab.remoteLogin.address;
        # wg0 may come up after the socket.
        FreeBind = true;
        IPAddressAllow = lab.remoteLogin.allow;
        IPAddressDeny = "any";
      };
    };
    networking.firewall.interfaces.wg0.allowedTCPPorts =
      lib.optional (lab.remoteLogin.address != null)
        (lib.toInt (lib.last (lib.splitString ":" lab.remoteLogin.address)))
      ++ lib.optional (lab.outbox.port != null) lab.outbox.port;

    systemd.services = {
      "${lab.unit}-login-remote" = lib.mkIf (lab.remoteLogin.address != null) {
        description = "Remote clients' way to the lab's login port";
        requires = [ "${lab.unit}-login-remote.socket" ];
        after = [ "${lab.unit}-login-remote.socket" ];
        serviceConfig = {
          ExecStart = "${config.systemd.package}/lib/systemd/systemd-socket-proxyd --exit-idle-time=10min 127.0.0.1:${toString lab.loginPort}";
          DynamicUser = true;
          PrivateTmp = true;
          PrivateDevices = true;
          ProtectHome = true;
          ProtectSystem = "strict";
          NoNewPrivileges = true;
          RestrictAddressFamilies = [ "AF_INET" "AF_UNIX" ];
          CapabilityBoundingSet = "";
        };
      };

      "${lab.unit}-control@" = {
        description = "Minecraft lab helper request";
        after = [ "minecraft-lab-setup.service" ];
        serviceConfig = {
          ExecStart = "${storage}/bin/minecraft-storage lab-socket ${name}";
          StandardInput = "socket";
          StandardOutput = "socket";
          StandardError = "journal";
        };
      };

      "${lab.unit}-fence" = {
        description = "Firewall for the Minecraft lab's traffic";
        wantedBy = [ "multi-user.target" ];
        before = [ "${lab.unit}-pasta.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${pkgs.nftables}/bin/nft -f ${fence lab}";
          ExecStop = "${pkgs.nftables}/bin/nft delete table inet ${lab.fenceTable}";
        };
      };

      # User-mode networking: pasta runs as the lab user and makes the namespace
      # itself (it can't re-enter a root-owned one after dropping privileges),
      # so every lab packet leaves as a socket of the lab user's, which the fence
      # restricts. Nothing is forwarded from the host into the namespace but the
      # login port, and nothing from the namespace to the host's ports. The
      # namespace is bind-mounted at lab.netns for the other lab units.
      "${lab.unit}-pasta" = {
        description = "Network namespace and internet access for the Minecraft lab";
        wantedBy = [ "multi-user.target" ];
        requires = [ "${lab.unit}-fence.service" ];
        after = [ "${lab.unit}-fence.service" "network-online.target" ];
        wants = [ "network-online.target" ];
        path = [ pkgs.coreutils pkgs.util-linux ];
        serviceConfig = {
          User = lab.user;
          Group = lab.user;
          RuntimeDirectory = "${lab.unit}-pasta";
          # --config-net copies the host's IPv4 setup once, at start, but
          # network-online can come from IPv6 alone: on 2026-10-01 pasta
          # started 3 s before dhcpcd's IPv4 lease, and the namespace had no
          # route until a restart. Wait for an IPv4 default route first.
          ExecStartPre = pkgs.writeShellScript "minecraft-lab-wait-ipv4" ''
            for _ in $(${pkgs.coreutils}/bin/seq 60); do
              [[ -n $(${pkgs.iproute2}/bin/ip -4 route show default) ]] && exit 0
              ${pkgs.coreutils}/bin/sleep 1
            done
            echo "no IPv4 default route after 60 s" >&2
            exit 1
          '';
          ExecStart = lib.escapeShellArgs [
            "${pkgs.passt}/bin/pasta" "--foreground" "--quiet" "--config-net" "--ipv4-only" "--no-map-gw"
            "-t" "127.0.0.1/${toString lab.loginPort}:25565" "-u" "none" "-T" "none" "-U" "none"
            "--" "${pkgs.coreutils}/bin/sleep" "infinity"
          ];
          # pasta's command (sleep) runs in the new namespaces; find it by
          # host pid, and never bind the host's own namespace by mistake.
          ExecStartPost = "+${pkgs.writeShellScript "${lab.unit}-netns" ''
            set -eu
            host=$(readlink /proc/1/ns/net)
            for _ in $(seq 100); do
              for pid in $(cat /proc/"$MAINPID"/task/*/children 2>/dev/null); do
                ns=$(readlink /proc/"$pid"/ns/net) || continue
                if [[ $ns != "$host" ]]; then
                  mkdir -p /run/netns
                  umount ${lab.netns} 2>/dev/null || true
                  touch ${lab.netns}
                  mount --bind /proc/"$pid"/ns/net ${lab.netns}
                  [[ $(stat -c %i ${lab.netns}) == "''${ns//[^0-9]/}" ]]
                  exit 0
                fi
              done
              sleep 0.1
            done
            echo "pasta's namespace not found" >&2
            exit 1
          ''}";
          ExecStopPost = "+-${pkgs.util-linux}/bin/umount ${lab.netns}";
          Restart = "on-failure";
          RestartSec = "10s";
        };
      };

      "${lab.unit}@" = {
        description = "Lab Minecraft server %i";
        bindsTo = [ "${lab.unit}-pasta.service" ];
        after = [ "${lab.unit}-pasta.service" "nix-daemon.socket" ];
        path = [ pkgs.bash config.nix.package pkgs.git ];
        environment = {
          # Launch Java directly (start.py's daily-restart thread comes with it).
          MINECRAFT_UNIT = "%n";
          # Off unless a clone's lab.env says CRASH_ANALYSIS=agent; then a crash
          # starts a turn for the lab's agent, as it would for tsugumi-minecraft.
          CRASH_ANALYSIS = "0";
          NIX_PATH = lib.concatStringsSep ":" config.nix.nixPath;
        } // lib.optionalAttrs (lab.agent != null) {
          CRASH_ANALYSIS_TRIGGER_DIR = "/var/lib/agent-bridge/${lab.agent}/triggers";
        };
        restartIfChanged = false;
        unitConfig = {
          RequiresMountsFor = "${lab.mounts}/%i";
          AssertPathIsDirectory = "${lab.mounts}/%i";
        };
        serviceConfig = lab.serviceConfig // {
          User = lab.user;
          Group = lab.user;
          Slice = "${lab.slice}.slice";
          WorkingDirectory = "${lab.mounts}/%i";
          # update-and-start.sh would link into /home/minecraft/builder.
          ExecStart = "${lab.mounts}/%i/server/start.py";
          # Per-clone overrides (e.g. CRASH_ANALYSIS=agent); these win over environment.
          EnvironmentFile = "-${lab.mounts}/%i/lab.env";
          ExecStop = "-${stop lab} %i";
          Restart = "no";
          TimeoutStopSec = "7min";
        };
      };

      # Self-healing for the lab's internet: probes from inside the namespace every 3 minutes
      # and restarts pasta after two failures (Baughn, msg 1555937291078344726).
      "${lab.unit}-netcheck" = {
        description = "Check the Minecraft lab's internet access, and restart pasta if it's gone";
        after = [ "${lab.unit}-pasta.service" ];
        requisite = [ "${lab.unit}-pasta.service" ];
        path = [ pkgs.util-linux pkgs.iproute2 pkgs.nftables config.systemd.package config.me.discordNotify.package ];
        serviceConfig = {
          Type = "oneshot";
          ExecStart = lib.escapeShellArgs ([
            "${pkgs.python3}/bin/python3" "-I" "${./minecraft-lab-netcheck.py}"
            lab.netns "/var/lib/${lab.unit}-netcheck" "--unit" lab.unit "--fence" lab.fenceTable
          ] ++ lib.optionals (lab.agent != null) [ "--bridge" "agent-bridge-${lab.agent}.service" ]);
          StateDirectory = "${lab.unit}-netcheck";
          SupplementaryGroups = [ "discord-notify" ];
          TimeoutStartSec = "5min";
        };
      };

      # The lab's card on the board's status page (agents.brage.info/status): clones and
      # running servers. Posts as the lab user, which the board knows as the lab's agent.
      "${lab.unit}-status" = lib.mkIf (lab.agent != null) {
        description = "Post the Minecraft lab's status to the agent board";
        after = [ "agent-board-api.socket" "minecraft-lab-setup.service" ];
        path = [ client pkgs.systemd ];
        serviceConfig = {
          Type = "oneshot";
          User = lab.user;
          Group = lab.user;
          ExecStart = lib.escapeShellArgs [
            "${pkgs.python3}/bin/python3" "-I" "${./minecraft-lab-status.py}" "/run/agent-board/api.sock"
            "--unit" lab.unit "--slice" "${lab.slice}.slice"
          ];
          ProtectSystem = "strict";
          ProtectHome = true;
          PrivateTmp = true;
          NoNewPrivileges = true;
          RestrictAddressFamilies = [ "AF_UNIX" ];
          TimeoutStartSec = 60;
        };
      };
    };

    systemd.timers."${lab.unit}-netcheck" = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "5min";
        OnUnitActiveSec = "3min";
      };
    };
    systemd.timers."${lab.unit}-status" = lib.mkIf (lab.agent != null) {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "2min";
        OnUnitActiveSec = "1min";
        AccuracySec = "10s";
      };
    };

    # Every lab server together; production uses -Xmx8G with AlwaysPreTouch.
    systemd.slices.${lab.slice}.sliceConfig = {
      MemoryMax = lab.memoryMax;
      CPUQuota = lab.cpuQuota;
    };

    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${lab.user}" &&
            /^${lib.escapeRegex lab.unit}@[a-z0-9][a-z0-9-]*\.service$/.test(action.lookup("unit")) &&
            ["start", "stop", "restart", "try-restart"].indexOf(action.lookup("verb")) >= 0) {
          return polkit.Result.YES;
        }
      });
    '';
  };
in
{
  options.me.minecraft.labs = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule labOptions);
    default = { };
    description = ''
      The Minecraft labs, by name. "main" is tsugumi-lab's and always exists;
      another lab needs at least loginPort.
    '';
  };

  # Top-level keys stay fixed: a list of per-lab configs at the top would make
  # the labs option depend on itself.
  config = let
    perLab = lib.mapAttrsToList labConfig labs;
    pick = key: lib.mkMerge (map (lab: lab.${key} or { }) perLab);
  in lib.mkMerge [
    { users = pick "users"; systemd = pick "systemd"; services = pick "services";
      networking = pick "networking"; security = pick "security"; }
    {
      me.minecraft.labs.main = { };

      assertions = lib.mapAttrsToList (name: _: {
        assertion = builtins.match "[a-z][a-z0-9]{0,15}" name != null && !(builtins.elem name reserved);
        message = "me.minecraft.labs.${name}: a lab's name is 1-16 of a-z and 0-9, not one of ${toString reserved}.";
      }) labs ++ [{
        assertion = lib.allUnique (lib.mapAttrsToList (_: lab: lab.loginPort) labs);
        message = "me.minecraft.labs: every lab needs its own loginPort.";
      }];

      users.groups.lab-handover.members = [ "minecraft" ];

      environment.systemPackages = [ client ];
      system.build.minecraft-lab-client = client;

      # Shared by every lab: minecraft-storage lab boot / lab expire cover them all.
      systemd.services = {
        # Creates the labs' parent datasets and remounts clones (legacy mounts) at boot.
        minecraft-lab-setup = {
          description = "Minecraft lab datasets";
          wantedBy = [ "multi-user.target" ];
          after = [ "zfs.target" "systemd-tmpfiles-setup.service" ];
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
            ExecStart = "${storage}/bin/minecraft-storage lab boot";
          };
        };

        minecraft-lab-expire = {
          description = "Destroy expired Minecraft lab clones";
          after = [ "minecraft-lab-setup.service" ];
          serviceConfig = {
            Type = "oneshot";
            ExecStart = "${storage}/bin/minecraft-storage lab expire";
          };
        };
      };

      systemd.timers.minecraft-lab-expire = {
        wantedBy = [ "timers.target" ];
        timerConfig = {
          OnCalendar = "hourly";
          Persistent = true;
        };
      };

      security.polkit.enable = true;
    }
  ];
}
