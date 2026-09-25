{ config, lib, pkgs, ... }:
# One system unit per Minecraft world, replacing tmux as the supervisor.
# See docs/agent-channel-design.md, "Server lifecycle".
let
  cfg = config.me.minecraft;

  # update-and-start.sh (#!/usr/bin/env bash) runs nix flake metadata / nix
  # build on the builder checkout, and start.py is a nix-shell script. Set on
  # instances too: their drop-ins carry a default PATH that overrides this one.
  path = [ pkgs.bash config.nix.package pkgs.git ];
  uid = toString config.users.users.minecraft.uid;

  # update-and-start.sh prompts when a directory isn't set up yet, and with
  # stdin on the console FIFO nothing would ever answer. Refuse instead, and
  # refuse a world whose start.py is already running elsewhere (e.g. tmux).
  guard = pkgs.writeShellApplication {
    name = "minecraft-start-guard";
    runtimeInputs = [ pkgs.coreutils pkgs.gnused ];
    text = ''
      dir=$(pwd)
      for need in world mods; do
        if [[ ! -d $need ]]; then
          echo "$dir/$need is missing; not a server directory" >&2
          exit 1
        fi
      done
      if [[ ! -f server.nix-target ]]; then
        echo "$dir/server.nix-target is missing; run update-and-start.sh by hand once" >&2
        exit 1
      fi
      pid=$(cat server.pid 2>/dev/null) || exit 0
      [[ $pid =~ ^[0-9]+$ ]] || exit 0
      script=$(tr '\0' '\n' 2>/dev/null < "/proc/$pid/cmdline" | sed -n 2p) || exit 0
      [[ $script == /* ]] || script=$(readlink "/proc/$pid/cwd" 2>/dev/null)/$script
      if [[ $script == "$dir/server/start.py" ]]; then
        echo "$dir is already running as pid $pid (in tmux?); stop it first" >&2
        exit 1
      fi
    '';
  };

  mcConsole = pkgs.writeShellApplication {
    name = "mc-console";
    runtimeInputs = [ pkgs.bash pkgs.coreutils pkgs.rlwrap pkgs.systemd ];
    text = ''
      if [[ $# -ne 1 || ! $1 =~ ^[a-z0-9-]+$ ]]; then
        echo "usage: mc-console WORLD" >&2
        exit 2
      fi
      fifo=/run/minecraft/$1.stdin
      if [[ ! -p $fifo ]]; then
        echo "minecraft@$1 is not running (no $fifo)" >&2
        exit 1
      fi
      history=''${XDG_STATE_HOME:-$HOME/.local/state}/mc-console
      mkdir -p "$history"
      # The journal tail runs under rlwrap, so rlwrap redraws the prompt around
      # server output instead of letting it garble half-typed input. Each line
      # reopens the FIFO, which is recreated whenever the world restarts.
      # shellcheck disable=SC2016
      exec rlwrap -H "$history/$1.history" -s 5000 -S "$1> " \
        bash -c 'journalctl -fu "minecraft@$1.service" -n 100 -o cat & tail=$!
                 trap "kill $tail" EXIT
                 while IFS= read -r line; do
                   printf "%s\n" "$line" 2>/dev/null > "$2" ||
                     echo "[mc-console] $1 is not running; dropped: $line" >&2
                 done' mc-console "$1" "$fifo"
    '';
  };
in
{
  options.me.minecraft.autostart = lib.mkOption {
    type = lib.types.listOf (lib.types.strMatching "[a-z0-9-]+");
    default = [ ];
    description = ''
      Worlds (directories under /home/minecraft) started at boot as
      minecraft@WORLD. Cut worlds over one at a time: a world must never run
      under tmux and this unit at once.
    '';
  };

  config = {
    systemd.sockets."minecraft@" = {
      description = "Console of Minecraft world %i";
      # The FIFO exists only while the world runs, so writing to it can never
      # start a stopped world. It is recreated on every restart, whichever
      # dependency is used (BindsTo, PartOf and StopPropagatedFrom all do,
      # per the VM test), so mc-console reopens it per line.
      bindsTo = [ "minecraft@%i.service" ];
      socketConfig = {
        ListenFIFO = "/run/minecraft/%i.stdin";
        SocketUser = "minecraft";
        SocketGroup = "users";
        SocketMode = "0600";
        RemoveOnStop = true;
        FlushPending = true;
      };
    };

    systemd.services = {
      "minecraft@" = {
        description = "Minecraft world %i";
        requires = [ "minecraft@%i.socket" ];
        wants = [ "network-online.target" "user@${uid}.service" ];
        after = [ "minecraft@%i.socket" "network-online.target" "nix-daemon.socket" "user@${uid}.service" ];
        inherit path;
        environment = {
          # Tells start.py it is supervised: launch Java directly, run extras.
          MINECRAFT_UNIT = "%n";
          # The start.py shebang is `nix-shell -p …`, which needs <nixpkgs>.
          NIX_PATH = lib.concatStringsSep ":" config.nix.nixPath;
          # Crash analysis escapes to the user manager via systemd-run --user,
          # so the restart that follows a crash doesn't kill it.
          XDG_RUNTIME_DIR = "/run/user/${uid}";
          DBUS_SESSION_BUS_ADDRESS = "unix:path=/run/user/${uid}/bus";
        };
        # A deploy must never restart a running world.
        restartIfChanged = false;
        unitConfig = {
          RequiresMountsFor = "/home/minecraft/%i";
          AssertPathIsDirectory = "/home/minecraft/%i";
          # Keep retrying, e.g. when nix build fails at boot; RestartSteps backs off.
          StartLimitIntervalSec = 0;
        };
        serviceConfig = {
          User = "minecraft";
          Group = "users";
          WorkingDirectory = "/home/minecraft/%i";
          ExecStartPre = lib.getExe guard;
          ExecStart = "/home/minecraft/%i/update-and-start.sh";
          ExecStop = "/home/minecraft/%i/control.sh stop -t 10";
          # Like the old update-and-loop.sh: a /stop in game restarts the world,
          # systemctl stop stops it.
          Restart = "always";
          RestartSec = "5s";
          RestartSteps = 5;
          RestartMaxDelaySec = "5min";
          # control stop: 10 s warning, save, then up to 300 s before it kills.
          TimeoutStopSec = "7min";
          StandardInput = "socket";
          StandardOutput = "journal";
          StandardError = "journal";
        };
      };
    } // lib.listToAttrs (map
      (world: lib.nameValuePair "minecraft@${world}" {
        overrideStrategy = "asDropin";
        inherit path;
        wantedBy = [ "multi-user.target" ];
        restartIfChanged = false;
      })
      cfg.autostart);

    systemd.tmpfiles.rules = [ "d /run/minecraft 0755 root root -" ];

    # minecraft (admins over ssh, and later the agent) manages its world units
    # and nothing else. Unlike sudo, this works under NoNewPrivileges.
    security.polkit.enable = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "minecraft" &&
            /^minecraft@[a-z0-9-]+\.service$/.test(action.lookup("unit")) &&
            ["start", "stop", "restart", "try-restart"].indexOf(action.lookup("verb")) >= 0) {
          return polkit.Result.YES;
        }
      });
    '';

    environment.systemPackages = [ mcConsole ];
  };
}
