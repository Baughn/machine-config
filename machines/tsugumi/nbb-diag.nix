{ config, lib, pkgs, ... }:

let
  diagPubKey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDM+kyNji+bKFCVgkhh3CwDJg2ihovEoZ/0hzBU7CQrU nbb-diag@saya";

  wrapper = pkgs.writeShellScript "nbb-daemon-wrap" ''
    # Diagnostic wrapper for ssh sessions arriving via the nbb-diag key.
    # Every session is captured (any SSH_ORIGINAL_COMMAND), so if the
    # protocol-mismatch bug recurs we can see exactly which commands ran
    # and what bytes flowed.
    #
    # CRITICAL: this script must NOT write to its own stdout/stderr until
    # exec — any leaked byte corrupts the protocol on the other end.

    set -u
    export PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.procps ]}:/run/current-system/sw/bin

    LOGDIR=/var/log/nbb-trace
    mkdir -p "$LOGDIR" 2>/dev/null
    TS=$(date -u +%Y%m%dT%H%M%S.%6N 2>/dev/null)
    LOG="$LOGDIR/$TS-$$"

    # Effective command: SSH_ORIGINAL_COMMAND if set, otherwise login shell.
    # We deny pty so a true interactive login is unlikely, but handle it.
    CMD="''${SSH_ORIGINAL_COMMAND:-}"
    if [ -z "$CMD" ]; then
      CMD="''${SHELL:-/bin/sh}"
    fi

    # One-line summary in a tail-able session log. printf of a short string
    # to an O_APPEND file is atomic under PIPE_BUF, so concurrent sessions
    # won't interleave.
    printf '%s pid=%s ppid=%s conn=%s cmd=%s\n' \
      "$TS" "$$" "$PPID" "''${SSH_CONNECTION:-?}" "$CMD" \
      >>"$LOGDIR/sessions.log" 2>/dev/null || true

    # Snapshot server state to a meta file. All ancillary commands redirect
    # into the meta file — none of this hits stdout.
    {
      echo "=== START $TS pid=$$ ppid=$PPID ==="
      echo "SSH_CONNECTION=''${SSH_CONNECTION:-}"
      echo "SSH_ORIGINAL_COMMAND=''${SSH_ORIGINAL_COMMAND:-}"
      echo "CMD=$CMD"
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

    # Tee every session regardless of command. nix-daemon, nix-store --serve,
    # interactive shell — all get captured. bash -c with a simple command
    # execs into it in-place, so there's no extra process layer for the
    # common nix-daemon path.
    exec ${pkgs.bash}/bin/bash -c "$CMD" \
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
