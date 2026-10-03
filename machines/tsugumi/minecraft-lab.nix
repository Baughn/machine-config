{ config, lib, pkgs, ... }:
# The Minecraft lab: writable ZFS clones of the worlds, and servers running on
# them, for the tsugumi-lab agent (user mclab). Clones are made by the root
# helper (minecraft-storage lab …), which mclab reaches only through
# minecraft-lab-control.socket. Lab servers and the agent share the network
# namespace mclab, linked to the internet by pasta and fenced off from the host
# and private addresses. See docs/agent-channel-design.md, "The lab identity".
let
  cfg = config.me.minecraft.lab;
  storage = config.system.build.minecraft-storage;
  netns = "/run/netns/mclab";

  client = pkgs.writeScriptBin "minecraft-lab" ''
    #!${pkgs.python3}/bin/python3 -I
    """minecraft-lab list | clone WORLD|rpool/minecraft/WORLD@SNAPSHOT NAME | destroy NAME"""
    import json, socket, sys
    connection = socket.socket(socket.AF_UNIX)
    connection.connect("/run/minecraft-lab.sock")
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

  # Output from mclab's sockets (pasta carries every lab packet, so all of them)
  # may go to the internet only: not to this host, not to private ranges. Replies
  # on connections others opened (the login port forward) pass. A positive match
  # and jump: packets without a socket (NDP, IGMP, WireGuard's kernel socket)
  # match no skuid and must fall through to accept, not reach the drops.
  fence = pkgs.writeText "mclab-fence.nft" ''
    table inet mclab-fence
    delete table inet mclab-fence
    table inet mclab-fence {
      chain output {
        type filter hook output priority filter; policy accept;
        meta skuid "${cfg.user}" jump lab
      }
      chain lab {
        ct direction reply accept
        meta nfproto ipv6 drop
        fib daddr type local drop
        ip daddr { ${lib.concatStringsSep ", " cfg.fenced} } drop
      }
    }
  '';

  stop = pkgs.writeShellScript "minecraft-lab-stop" ''
    [[ -n ''${MAINPID:-} ]] || exit 0
    exec "/srv/minecraft-lab/$1/control.sh" stop -t 10
  '';
in
{
  options.me.minecraft.lab = {
    user = lib.mkOption { type = lib.types.str; default = "mclab"; readOnly = true; };
    maxClones = lib.mkOption { type = lib.types.int; default = 3; };
    lifetime = lib.mkOption {
      type = lib.types.int;
      default = 5 * 86400;
      description = "Seconds until a clone expires; shorter than the SSD's 7-day snapshot retention.";
    };
    quota = lib.mkOption { type = lib.types.str; default = "200G"; description = "Quota on rpool/minecraft-lab."; };
    scrub = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.str);
      default = {
        "config/Discord-Integration.toml" = [ "botToken" ];
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
    loginPort = lib.mkOption {
      type = lib.types.port;
      default = 25665;
      description = "Host 127.0.0.1 port forwarded to the lab's 25565: ssh -L 25565:localhost:<port> tsugumi.";
    };
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
    serviceConfig = lib.mkOption {
      type = lib.types.attrsOf lib.types.anything;
      readOnly = true;
      description = "Sandbox shared by the lab's units (the agent bridge and minecraft-lab@).";
      default = {
        NetworkNamespacePath = netns;
        # Hides /home/minecraft (others may read it) and every other home.
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        BindReadOnlyPaths = [ "${resolvConf}:/etc/resolv.conf" ];
      };
    };
  };

  config = {
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.user;
      home = "/var/lib/${cfg.user}";
      createHome = true;
      homeMode = "700";
      description = "Minecraft lab agent";
    };
    users.groups.${cfg.user} = { };

    # Handover from the lab to production (mods, configs, instructions): the
    # lab writes, tsugumi-minecraft reads. setgid keeps new entries in the
    # group; files copied with mode 0600 still need chmod g+r.
    # The lab is a member too, so it can chgrp what it makes there (cp keeps
    # group mclab; the bridge's umask makes files 0600).
    users.groups.lab-handover.members = [ "minecraft" cfg.user ];

    environment.systemPackages = [ client ];
    system.build.minecraft-lab-client = client;

    systemd.tmpfiles.rules = [
      "d /srv/minecraft-lab 0755 root root -"
      "d /srv/lab-handover 2750 ${cfg.user} lab-handover -"
    ];

    systemd.sockets.minecraft-lab-control = {
      description = "Minecraft lab helper";
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = "/run/minecraft-lab.sock";
        SocketUser = "root";
        SocketGroup = cfg.user;
        SocketMode = "0660";
        Accept = true;
        MaxConnections = 4;
      };
    };

    # The login port for remote clients: saya's client-tester agent joins lab servers here.
    # A socket unit's IP access list applies to its listening socket (so to who may connect),
    # not to the proxy's own connection to the login port. The proxy runs outside the lab
    # and its fence; it only ever reaches pasta's host-side listener.
    systemd.sockets.minecraft-lab-login-remote = lib.mkIf (cfg.remoteLogin.address != null) {
      description = "Remote clients' way to the lab's login port";
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = cfg.remoteLogin.address;
        # wg0 may come up after the socket.
        FreeBind = true;
        IPAddressAllow = cfg.remoteLogin.allow;
        IPAddressDeny = "any";
      };
    };
    networking.firewall.interfaces.wg0.allowedTCPPorts = lib.mkIf (cfg.remoteLogin.address != null)
      [ (lib.toInt (lib.last (lib.splitString ":" cfg.remoteLogin.address))) ];

    systemd.services = {
      minecraft-lab-login-remote = lib.mkIf (cfg.remoteLogin.address != null) {
        description = "Remote clients' way to the lab's login port";
        requires = [ "minecraft-lab-login-remote.socket" ];
        after = [ "minecraft-lab-login-remote.socket" ];
        serviceConfig = {
          ExecStart = "${config.systemd.package}/lib/systemd/systemd-socket-proxyd --exit-idle-time=10min 127.0.0.1:${toString cfg.loginPort}";
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

      "minecraft-lab-control@" = {
        description = "Minecraft lab helper request";
        after = [ "minecraft-lab-setup.service" ];
        serviceConfig = {
          ExecStart = "${storage}/bin/minecraft-storage lab-socket";
          StandardInput = "socket";
          StandardOutput = "socket";
          StandardError = "journal";
        };
      };

      # Creates rpool/minecraft-lab and remounts clones (legacy mounts) at boot.
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

      minecraft-lab-fence = {
        description = "Firewall for the Minecraft lab's traffic";
        wantedBy = [ "multi-user.target" ];
        before = [ "minecraft-lab-pasta.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${pkgs.nftables}/bin/nft -f ${fence}";
          ExecStop = "${pkgs.nftables}/bin/nft delete table inet mclab-fence";
        };
      };

      # User-mode networking: pasta runs as mclab and makes the namespace
      # itself (it can't re-enter a root-owned one after dropping privileges),
      # so every lab packet leaves as a socket of mclab's, which the fence
      # restricts. Nothing is forwarded from the host into the namespace but the
      # login port, and nothing from the namespace to the host's ports. The
      # namespace is bind-mounted at ${netns} for the other lab units.
      minecraft-lab-pasta = {
        description = "Network namespace and internet access for the Minecraft lab";
        wantedBy = [ "multi-user.target" ];
        requires = [ "minecraft-lab-fence.service" ];
        after = [ "minecraft-lab-fence.service" "network-online.target" ];
        wants = [ "network-online.target" ];
        path = [ pkgs.coreutils pkgs.util-linux ];
        serviceConfig = {
          User = cfg.user;
          Group = cfg.user;
          RuntimeDirectory = "minecraft-lab-pasta";
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
            "-t" "127.0.0.1/${toString cfg.loginPort}:25565" "-u" "none" "-T" "none" "-U" "none"
            "--" "${pkgs.coreutils}/bin/sleep" "infinity"
          ];
          # pasta's command (sleep) runs in the new namespaces; find it by
          # host pid, and never bind the host's own namespace by mistake.
          ExecStartPost = "+${pkgs.writeShellScript "minecraft-lab-netns" ''
            set -eu
            host=$(readlink /proc/1/ns/net)
            for _ in $(seq 100); do
              for pid in $(cat /proc/"$MAINPID"/task/*/children 2>/dev/null); do
                ns=$(readlink /proc/"$pid"/ns/net) || continue
                if [[ $ns != "$host" ]]; then
                  mkdir -p /run/netns
                  umount ${netns} 2>/dev/null || true
                  touch ${netns}
                  mount --bind /proc/"$pid"/ns/net ${netns}
                  [[ $(stat -c %i ${netns}) == "''${ns//[^0-9]/}" ]]
                  exit 0
                fi
              done
              sleep 0.1
            done
            echo "pasta's namespace not found" >&2
            exit 1
          ''}";
          ExecStopPost = "+-${pkgs.util-linux}/bin/umount ${netns}";
          Restart = "on-failure";
          RestartSec = "10s";
        };
      };

      "minecraft-lab@" = {
        description = "Lab Minecraft server %i";
        bindsTo = [ "minecraft-lab-pasta.service" ];
        after = [ "minecraft-lab-pasta.service" "nix-daemon.socket" ];
        path = [ pkgs.bash config.nix.package pkgs.git ];
        environment = {
          # Launch Java directly (start.py's daily-restart thread comes with it).
          MINECRAFT_UNIT = "%n";
          # Off unless a clone's lab.env says CRASH_ANALYSIS=agent; then a crash
          # starts a turn for tsugumi-lab, as it would for tsugumi-minecraft.
          CRASH_ANALYSIS = "0";
          CRASH_ANALYSIS_TRIGGER_DIR = "/var/lib/agent-bridge/tsugumi-lab/triggers";
          NIX_PATH = lib.concatStringsSep ":" config.nix.nixPath;
        };
        restartIfChanged = false;
        unitConfig = {
          RequiresMountsFor = "/srv/minecraft-lab/%i";
          AssertPathIsDirectory = "/srv/minecraft-lab/%i";
        };
        serviceConfig = cfg.serviceConfig // {
          User = cfg.user;
          Group = cfg.user;
          Slice = "mclab.slice";
          WorkingDirectory = "/srv/minecraft-lab/%i";
          # update-and-start.sh would link into /home/minecraft/builder.
          ExecStart = "/srv/minecraft-lab/%i/server/start.py";
          # Per-clone overrides (e.g. CRASH_ANALYSIS=agent); these win over environment.
          EnvironmentFile = "-/srv/minecraft-lab/%i/lab.env";
          ExecStop = "-${stop} %i";
          Restart = "no";
          TimeoutStopSec = "7min";
        };
      };
    };

    # Self-healing for the lab's internet: probes from inside the namespace every 3 minutes
    # and restarts pasta after two failures (Baughn, msg 1555937291078344726).
    systemd.services.minecraft-lab-netcheck = {
      description = "Check the Minecraft lab's internet access, and restart pasta if it's gone";
      after = [ "minecraft-lab-pasta.service" ];
      requisite = [ "minecraft-lab-pasta.service" ];
      path = [ pkgs.util-linux pkgs.iproute2 pkgs.nftables config.systemd.package config.me.discordNotify.package ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${pkgs.python3}/bin/python3 -I ${./minecraft-lab-netcheck.py} ${netns} /var/lib/minecraft-lab-netcheck";
        StateDirectory = "minecraft-lab-netcheck";
        SupplementaryGroups = [ "discord-notify" ];
        TimeoutStartSec = "5min";
      };
    };
    systemd.timers.minecraft-lab-netcheck = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "5min";
        OnUnitActiveSec = "3min";
      };
    };

    # The lab's card on the board's status page (agents.brage.info/status): clones and
    # running servers. Posts as the lab user, which the board knows as tsugumi-lab.
    systemd.services.minecraft-lab-status = {
      description = "Post the Minecraft lab's status to the agent board";
      after = [ "agent-board-api.socket" "minecraft-lab-setup.service" ];
      path = [ client pkgs.systemd ];
      serviceConfig = {
        Type = "oneshot";
        User = cfg.user;
        Group = cfg.user;
        ExecStart = "${pkgs.python3}/bin/python3 -I ${./minecraft-lab-status.py} /run/agent-board/api.sock";
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        TimeoutStartSec = 60;
      };
    };
    systemd.timers.minecraft-lab-status = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "2min";
        OnUnitActiveSec = "1min";
        AccuracySec = "10s";
      };
    };

    systemd.timers.minecraft-lab-expire = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "hourly";
        Persistent = true;
      };
    };

    # Every lab server together; production uses -Xmx8G with AlwaysPreTouch.
    systemd.slices.mclab.sliceConfig = {
      MemoryMax = "16G";
      CPUQuota = "800%";
    };

    security.polkit.enable = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${cfg.user}" &&
            /^minecraft-lab@[a-z0-9][a-z0-9-]*\.service$/.test(action.lookup("unit")) &&
            ["start", "stop", "restart", "try-restart"].indexOf(action.lookup("verb")) >= 0) {
          return polkit.Result.YES;
        }
      });
    '';
  };
}
