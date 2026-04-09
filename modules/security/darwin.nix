{ lib, ... }:

{
  options.me.security = {
    enable = lib.mkEnableOption "system hardening";
  };

  # No-op on macOS. The NixOS-specific hardening (sysctl, fail2ban, auditd)
  # does not apply to Darwin.
}
