{ config, lib, pkgs, ... }:

let
  cfg = config.me.redis;
in
{
  options.me.redis = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable Redis server with custom configuration";
    };

    maxMemory = lib.mkOption {
      type = lib.types.str;
      default = "2gb";
      description = "Maximum memory Redis can use before evicting keys";
    };

    interface = lib.mkOption {
      type = lib.types.str;
      default = "wg0";
      description = "Network interface to bind Redis to";
    };
  };

  config = lib.mkIf cfg.enable {
    # Ensure we have the Redis password secret
    age.secrets."redis-password" = {
      file = ../secrets/redis-password.age;
      mode = "0400";
      owner = "redis-default";
      group = "redis-default";
    };

    # Configure Redis using the built-in NixOS module
    services.redis.servers.default = {
      enable = true;

      # Network configuration - bind to WireGuard interface
      # We need to get the actual IP from the interface
      bind =
        if config.networking.hostName == "tsugumi" then "10.171.0.2"
        else if config.networking.hostName == "saya" then "10.171.0.1"
        else "127.0.0.1";

      port = 6379;

      # Authentication
      requirePassFile = config.age.secrets."redis-password".path;

      # Persistence configuration
      save = [
        [ 900 1 ] # Save after 900 sec (15 min) if at least 1 key changed
        [ 300 10 ] # Save after 300 sec (5 min) if at least 10 keys changed  
        [ 60 10000 ] # Save after 60 sec if at least 10000 keys changed
      ];

      # Enable AOF for better durability
      appendOnly = true;
      appendFsync = "everysec"; # Sync to disk every second

      # Database settings
      databases = 16;

      # Logging
      logLevel = "notice";
      syslog = true;

      # Performance and limits
      settings = {
        # Memory management
        maxmemory = cfg.maxMemory;
        "maxmemory-policy" = "allkeys-lru"; # Evict least recently used keys when memory limit is reached

        # Connection settings
        timeout = 300; # Disconnect idle clients after 300 seconds
        "tcp-backlog" = 511;
        "tcp-keepalive" = 300;

        # Slow log - log queries slower than 10ms
        "slowlog-log-slower-than" = 10000; # microseconds
        "slowlog-max-len" = 128;

        # Disable dangerous commands in production
        "rename-command" = [
          "FLUSHDB ''"
          "FLUSHALL ''"
          "KEYS ''"
          "CONFIG ''"
        ];

        # Enable active rehashing for better performance
        "activerehashing" = "yes";

        # Client output buffer limits
        "client-output-buffer-limit" = [
          "normal 0 0 0"
          "replica 256mb 64mb 60"
          "pubsub 32mb 8mb 60"
        ];
      };
    };

    # Ensure Redis starts after WireGuard
    systemd.services.redis-default = {
      after = [ "systemd-networkd-wait-online.service" ];
      wants = [ "systemd-networkd-wait-online.service" ];
    };
  };
}
