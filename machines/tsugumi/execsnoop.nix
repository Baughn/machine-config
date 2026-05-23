{ config, lib, pkgs, ... }:

# Debug aid: log every exec() on tsugumi to /var/log/execsnoop.log via
# bcc's execsnoop, so we can correlate processes with the nbb protocol-
# mismatch bug captured in /var/log/nbb-trace/. Remove this module along
# with nbb-diag.nix once the root cause is identified.

let
  # bpftrace via tracepoints. bcc's execsnoop fails on this kernel because
  # the sys_execve kprobe target is inlined/notrace; tracepoints work.
  prog = pkgs.writeText "execsnoop.bt" ''
    BEGIN { printf("%-26s %-7s %-7s %s\n", "TIME", "PID", "PPID", "ARGS"); }
    tracepoint:syscalls:sys_enter_exec* {
      printf("%s %-7d %-7d ", strftime("%Y-%m-%dT%H:%M:%S.%f", nsecs), pid, ppid);
      join(args.argv);
    }
  '';

  runner = pkgs.writeShellScript "execsnoop-logger" ''
    set -u
    exec ${pkgs.bpftrace}/bin/bpftrace -B line ${prog}
  '';
in
{
  systemd.services.execsnoop = {
    description = "BPF exec tracer (debug for nbb protocol-mismatch bug)";
    wantedBy = [ "multi-user.target" ];
    after = [ "local-fs.target" ];
    serviceConfig = {
      Type = "simple";
      ExecStart = "${runner}";
      StandardOutput = "append:/var/log/execsnoop.log";
      StandardError = "append:/var/log/execsnoop.log";
      Restart = "always";
      RestartSec = 5;
    };
  };

  systemd.tmpfiles.rules = [
    "f /var/log/execsnoop.log 0644 root root -"
  ];

  services.logrotate.settings.execsnoop = {
    files = "/var/log/execsnoop.log";
    frequency = "daily";
    rotate = 7;
    compress = true;
    # execsnoop keeps the fd open; copytruncate avoids creating an
    # orphaned inode that the writer keeps appending to.
    copytruncate = true;
    missingok = true;
    notifempty = true;
  };
}
