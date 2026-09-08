{ config, lib, pkgs, ... }:
let
  cfg = config.me.minecraft;
  access = cfg.access;
  python = pkgs.python3.withPackages (p: [ p.aiohttp ]);
  source = pkgs.runCommand "minecraft-access-source" { } ''
    mkdir -p "$out"
    cp ${./minecraft-access}/* "$out/"
    substituteInPlace "$out/firewall.py" --replace-fail '@nft@' '${pkgs.nftables}/bin/nft'
  '';
  settings = pkgs.writeText "minecraft-access.json" (builtins.toJSON {
    inherit (cfg) ports;
    inherit (access) hostname ipv4Hostname ipv6Hostname clientId guildId roleId;
    leaseSeconds = access.leaseSeconds;
    sessionSeconds = 90 * 24 * 60 * 60;
    maxAddresses = 16;
  });
  broker = "${python}/bin/python3 ${source}/firewall.py ${settings}";
  hardening = {
    NoNewPrivileges = true;
    ProtectSystem = "strict";
    ProtectHome = true;
    PrivateTmp = true;
    ProtectKernelTunables = true;
    ProtectKernelModules = true;
    ProtectControlGroups = true;
    RestrictNamespaces = true;
    RestrictSUIDSGID = true;
    LockPersonality = true;
    MemoryDenyWriteExecute = true;
    UMask = "0007";
    Restart = "on-failure";
    MemoryMax = "256M";
    TasksMax = 32;
  };
  proxy = ''
    reverse_proxy unix//run/minecraft-access/http.sock {
      header_up X-Minecraft-Client-IP {remote_host}
    }
  '';
in
{
  options.me.minecraft = {
    ports = {
      tcp = lib.mkOption { type = lib.types.listOf lib.types.port; default = []; };
      udp = lib.mkOption { type = lib.types.listOf lib.types.port; default = []; };
    };
    access = {
      enable = lib.mkEnableOption "Discord-authorized Minecraft network access";
      hostname = lib.mkOption { type = lib.types.str; default = "minecraft.brage.info"; };
      ipv4Hostname = lib.mkOption { type = lib.types.str; default = "v4.brage.info"; };
      ipv6Hostname = lib.mkOption { type = lib.types.str; default = "v6.brage.info"; };
      clientId = lib.mkOption { type = lib.types.str; default = ""; };
      guildId = lib.mkOption { type = lib.types.str; default = ""; };
      roleId = lib.mkOption { type = lib.types.str; default = ""; };
      clientSecretFile = lib.mkOption {
        type = lib.types.str;
        default = "/run/agenix/minecraft-access-discord-secret";
        description = "Runtime credential file containing the Discord OAuth client secret.";
      };
      leaseSeconds = lib.mkOption {
        type = lib.types.ints.between 1 (14 * 24 * 60 * 60);
        default = 14 * 24 * 60 * 60;
        description = "Access duration; may be shortened for VM tests.";
      };
    };
  };

  config = lib.mkMerge [
    {
      # The independent guard below drops unlisted sources before the base
      # firewall. Its accepts are needed so listed traffic can pass both layers.
      networking.firewall.allowedTCPPorts = cfg.ports.tcp;
      networking.firewall.allowedUDPPorts = cfg.ports.udp;
    }
    (lib.mkIf access.enable {
      assertions = [
        {
          assertion = config.networking.firewall.enable && !config.networking.nftables.enable;
          message = "Minecraft access currently integrates with the iptables NixOS firewall.";
        }
        {
          assertion = builtins.all (id: builtins.match "[0-9]{1,20}" id != null)
            [ access.clientId access.guildId access.roleId ];
          message = "Minecraft access requires Discord client, guild, and role IDs.";
        }
        {
          assertion = builtins.all (host: builtins.match "[a-zA-Z0-9.-]+" host != null)
            [ access.hostname access.ipv4Hostname access.ipv6Hostname ]
            && builtins.length (lib.unique [ access.hostname access.ipv4Hostname access.ipv6Hostname ]) == 3;
          message = "Minecraft access hostnames must be distinct DNS names.";
        }
      ];
      users.groups.minecraft-access = {};
      users.groups.minecraft-access-http = {};
      users.users.minecraft-access = {
        isSystemUser = true;
        group = "minecraft-access";
        extraGroups = [ "minecraft-access-http" ];
      };
      users.users.caddy.extraGroups = [ "minecraft-access-http" ];

      networking.firewall.extraCommands = "${broker} --restore";
      # Keep the guard across firewall stop and atomically replace it on reload.
      systemd.services.minecraft-access-firewall = {
        description = "Minecraft address grant broker";
        wantedBy = [ "multi-user.target" ];
        requires = [ "firewall.service" ];
        after = [ "firewall.service" ];
        serviceConfig = hardening // {
          ExecStart = broker;
          Group = "minecraft-access";
          RuntimeDirectory = "minecraft-access-firewall";
          RuntimeDirectoryMode = "0750";
          StateDirectory = "minecraft-access-firewall";
          StateDirectoryMode = "0700";
          CapabilityBoundingSet = [ "CAP_NET_ADMIN" ];
          RestrictAddressFamilies = [ "AF_UNIX" "AF_NETLINK" ];
        };
      };
      systemd.services.minecraft-access = {
        description = "Minecraft Discord access frontend";
        wantedBy = [ "multi-user.target" ];
        wants = [ "minecraft-access-firewall.service" ];
        after = [ "minecraft-access-firewall.service" "network.target" ];
        environment.SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        serviceConfig = hardening // {
          ExecStart = "${python}/bin/python3 ${source}/server.py ${settings}";
          User = "minecraft-access";
          Group = "minecraft-access-http";
          SupplementaryGroups = [ "minecraft-access" ];
          RuntimeDirectory = "minecraft-access";
          RuntimeDirectoryMode = "0750";
          StateDirectory = "minecraft-access";
          StateDirectoryMode = "0700";
          LoadCredential = "discord-secret:${access.clientSecretFile}";
          CapabilityBoundingSet = "";
          RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];
        };
      };
      services.caddy = {
        enable = true;
        # Caddy's error logger also includes the request URI, which can contain
        # an OAuth code when the backend is down. Access-log suppression alone
        # does not cover that path.
        logFormat = lib.mkAfter ''
          level ERROR
          exclude http.log.error.minecraft_access_main http.log.error.minecraft_access_v4 http.log.error.minecraft_access_v6
        '';
        virtualHosts = {
          ${access.hostname} = {
            logFormat = null;
            extraConfig = ''
              log minecraft_access_main {
                output discard
              }
              ${proxy}
            '';
          };
        } // lib.genAttrs [ access.ipv4Hostname access.ipv6Hostname ] (host: {
          logFormat = null;
          extraConfig = ''
            log minecraft_access_${if host == access.ipv4Hostname then "v4" else "v6"} {
              output discard
            }
            handle /minecraft-access/authorize {
              ${proxy}
            }
            respond 404
          '';
        });
      };
    })
  ];
}
