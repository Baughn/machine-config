{ config, pkgs, ... }:

{
  networking.firewall.interfaces.wg0.allowedTCPPorts = [ 6379 ];

  services.redis.servers.default = {
    enable = true;
    bind = "0.0.0.0";
    port = 6379;
    requirePassFile = config.age.secrets."redis-password".path;
    save = [
      [ 900 1 ]
      [ 300 10 ]
      [ 60 10000 ]
    ];
    appendOnly = true;
    appendFsync = "everysec";
    databases = 16;
    logLevel = "notice";
    syslog = true;
    settings = {
      maxmemory = "2gb";
      "maxmemory-policy" = "volatile-lru";
      timeout = 300;
      "tcp-backlog" = 511;
      "tcp-keepalive" = 300;
      "slowlog-log-slower-than" = 10000;
      "slowlog-max-len" = 128;
      "rename-command" = [
        "FLUSHDB ''"
        "FLUSHALL ''"
        "CONFIG ''"
      ];
      "activerehashing" = "yes";
      "client-output-buffer-limit" = [
        "normal 0 0 0"
        "replica 256mb 64mb 60"
        "pubsub 32mb 8mb 60"
      ];
    };
  };

  systemd.services.redis-default = {
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
  };

  systemd.services.redis-nixcheck-acl = {
    description = "Setup Redis ACL for nixcheck user";
    wantedBy = [ "multi-user.target" ];
    after = [ "redis-default.service" ];
    wants = [ "redis-default.service" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      User = "root";
    };
    script = ''
      set -euo pipefail

      timeout=30
      while ! ${pkgs.redis}/bin/redis-cli -h 10.171.0.1 -p 6379 -a "$(cat ${config.age.secrets."redis-password".path})" ping > /dev/null 2>&1; do
        sleep 1
        timeout=$((timeout - 1))
        if [ "$timeout" -eq 0 ]; then
          echo "Timeout waiting for Redis to start"
          exit 1
        fi
      done

      NIXCHECK_PASSWORD=$(cat ${config.age.secrets."redis-nixcheck-password".path})
      REDIS_PASSWORD=$(cat ${config.age.secrets."redis-password".path})

      ${pkgs.redis}/bin/redis-cli -h 10.171.0.1 -p 6379 -a "$REDIS_PASSWORD" ACL SETUSER nixcheck \
        on \
        ">$NIXCHECK_PASSWORD" \
        "~nix-check:*" \
        "+get" "+set" "+setex" "+exists" "+del" "+ping" \
        || true
    '';
  };
}
