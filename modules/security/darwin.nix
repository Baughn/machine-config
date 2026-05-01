{ config, lib, ... }:

{
  options.me.security = {
    enable = lib.mkEnableOption "system hardening";
  };

  config.assertions = [{
    assertion = !config.me.security.enable;
    message = "me.security.enable is Linux-only — sysctl/fail2ban/auditd hardening doesn't apply on macOS.";
  }];
}
