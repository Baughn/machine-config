{ config, lib, ... }:

let
  cfg = config.me.security;
in
{
  options.me.security = {
    enable = lib.mkEnableOption "system hardening";
  };

  config = lib.mkIf cfg.enable {

    # -- Kernel sysctl hardening --
    boot.kernel.sysctl = {
      "kernel.dmesg_restrict" = 1;
      "kernel.perf_event_paranoid" = 3;
      "kernel.unprivileged_bpf_disabled" = 1;
      "net.core.bpf_jit_harden" = 2;
      "kernel.yama.ptrace_scope" = 1;
      "fs.protected_hardlinks" = 1;
      "fs.protected_symlinks" = 1;
      "fs.protected_fifos" = 2;
      "fs.protected_regular" = 2;
      # Network hardening
      "net.ipv4.conf.all.accept_redirects" = 0;
      "net.ipv4.conf.default.accept_redirects" = 0;
      "net.ipv6.conf.all.accept_redirects" = 0;
      "net.ipv6.conf.default.accept_redirects" = 0;
      "net.ipv4.conf.all.send_redirects" = 0;
      "net.ipv4.conf.default.send_redirects" = 0;
      "net.ipv4.conf.all.accept_source_route" = 0;
      "net.ipv4.conf.default.accept_source_route" = 0;
      "net.ipv6.conf.all.accept_source_route" = 0;
      "net.ipv6.conf.default.accept_source_route" = 0;
      "net.ipv4.conf.all.log_martians" = 1;
      "net.ipv4.conf.default.log_martians" = 1;
      "net.ipv4.icmp_echo_ignore_broadcasts" = 1;
      "net.ipv4.conf.all.rp_filter" = 1;
      "net.ipv4.conf.default.rp_filter" = 1;
    };

    # Blacklist uncommon/risky kernel modules
    boot.blacklistedKernelModules = [
      "dccp" "sctp" "rds" "tipc" "n-hdlc"
      "ax25" "netrom" "x25" "rose" "decnet" "econet"
      "af_802154" "ipx" "appletalk" "psnap" "p8023" "p8022"
      "can" "atm"
      "cramfs" "freevxfs" "jffs2" "hfs" "hfsplus" "udf"
    ];

    # -- fail2ban --
    services.fail2ban = {
      enable = true;
      maxretry = 3;
      bantime = "1h";
      bantime-increment = {
        enable = true;
        maxtime = "168h";
      };
      jails = {
        sshd.settings = {
          enabled = true;
          maxretry = 3;
          findtime = "10m";
        };
      };
    };

    # -- Audit framework --
    security.auditd.enable = true;
    security.audit = {
      enable = true;
      rules = [
        "-w /etc/sudoers -p wa -k sudoers"
        "-w /etc/sudoers.d/ -p wa -k sudoers"
        "-w /var/log/faillog -p wa -k auth"
        "-w /var/log/lastlog -p wa -k auth"
        "-w /etc/passwd -p wa -k identity"
        "-w /etc/group -p wa -k identity"
        "-w /etc/shadow -p wa -k identity"
        "-w /etc/hosts -p wa -k network"
      ];
    };

    # -- Journal retention for forensics (overrides cachy-tweaks 50MB) --
    services.journald.extraConfig = lib.mkForce ''
      SystemMaxUse=2G
      MaxRetentionSec=90day
    '';

    # Passwordless sudo scoped to svein only (not the entire wheel group).
    security.sudo.wheelNeedsPassword = true;
    security.sudo.extraRules = [{
      users = [ "svein" ];
      commands = [{ command = "ALL"; options = [ "NOPASSWD" ]; }];
    }];
  };
}
