{ config, lib, pkgs, ... }:

let
  diagPubKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDM+kyNji+bKFCVgkhh3CwDJg2ihovEoZ/0hzBU7CQrU nbb-diag@saya";

  wrapper = pkgs.writeShellScript "nbb-daemon-wrap" ''
    # Diagnostic wrapper for nix-daemon over ssh-ng.
    # Captures stdin/stdout/stderr to /var/log/nbb-trace to identify the source
    # of bytes being written to ssh stdout before nix-daemon's WORKER_MAGIC_2.
    #
    # CRITICAL: this script must NOT write to its own stdout until it has
    # exec'd nix-daemon — any byte we leak corrupts the daemon protocol.

    set -u
    export PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.procps ]}:/run/current-system/sw/bin

    LOGDIR=/var/log/nbb-trace
    mkdir -p "$LOGDIR" 2>/dev/null
    TS=$(date -u +%Y%m%dT%H%M%S.%6N 2>/dev/null)
    LOG="$LOGDIR/$TS-$$"

    # Snapshot server state to a meta file. All ancillary commands redirect
    # into the meta file — none of this hits stdout.
    {
      echo "=== START $TS pid=$$ ppid=$PPID ==="
      echo "SSH_CONNECTION=''${SSH_CONNECTION:-}"
      echo "SSH_ORIGINAL_COMMAND=''${SSH_ORIGINAL_COMMAND:-}"
      echo "PATH=$PATH"
      echo "--- ps -efH ---"
      ps -efH
      echo "--- ls /proc/$$/fd ---"
      ls -la /proc/$$/fd
      echo "--- ls /proc/$PPID/fd ---"
      ls -la /proc/$PPID/fd
      echo "--- /proc/$$/status ---"
      cat /proc/$$/status
      echo "--- /proc/$PPID/status ---"
      cat /proc/$PPID/status
      echo "=== BEGIN STREAM ==="
    } >"$LOG.meta" 2>&1

    # If this key is being used for something other than nix-daemon, just
    # dispatch the original command directly without tracing.
    if [ "''${SSH_ORIGINAL_COMMAND:-}" != "nix-daemon --stdio" ]; then
      if [ -n "''${SSH_ORIGINAL_COMMAND:-}" ]; then
        exec ${pkgs.bash}/bin/bash -c "''${SSH_ORIGINAL_COMMAND}"
      else
        exec "''${SHELL:-/bin/sh}"
      fi
    fi

    # Tee both directions. stderr also passes through to the client so any
    # nix-daemon error reaches saya — we just observe it.
    exec /run/current-system/sw/bin/nix-daemon --stdio \
      < <(tee "$LOG.in.bin") \
      1> >(tee "$LOG.out.bin") \
      2> >(tee "$LOG.err.bin" >&2)
  '';
in
{
  users.users.svein.openssh.authorizedKeys.keys = [
    ''command="${wrapper}",no-agent-forwarding,no-port-forwarding,no-X11-forwarding,no-pty ${diagPubKey}''
  ];

  systemd.tmpfiles.rules = [
    "d /var/log/nbb-trace 0755 svein root -"
    "e /var/log/nbb-trace - - - 7d"
  ];
}
