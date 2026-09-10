{
  config,
  lib,
  pkgs,
  ...
}:
let
  access = config.me.punch;
  localGroups = lib.filterAttrs (_: g: g.host == access.firewallHost) access.groups;
  python = pkgs.python3.withPackages (p: [ p.aiohttp ]);
  source = pkgs.runCommand "punch-source" { } ''
    mkdir -p "$out"
    cp ${./punch}/* "$out/"
    substituteInPlace "$out/firewall.py" --replace-fail '@nft@' '${pkgs.nftables}/bin/nft'
  '';
  settings = pkgs.writeText "punch.json" (
    builtins.toJSON {
      inherit (access)
        groups
        hostname
        ipv4Hostname
        ipv6Hostname
        clientId
        guildId
        firewallHost
        ;
      remoteBrokers = lib.mapAttrs (_: b: { inherit (b) url; }) access.remoteBrokers;
      leaseSeconds = access.leaseSeconds;
      sessionSeconds = 90 * 24 * 60 * 60;
      maxAddresses = 16;
    }
  );
  brokerSettings = pkgs.writeText "punch-firewall.json" (
    builtins.toJSON {
      groups = localGroups;
      inherit (access)
        leaseSeconds
        listenAddress
        listenPort
        ;
      maxAddresses = 16;
    }
  );
  broker = "${python}/bin/python3 ${source}/firewall.py ${brokerSettings}";
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
    reverse_proxy unix//run/punch/http.sock {
      header_up X-Punch-Client-IP {remote_host}
    }
  '';
in
{
  options.me.punch = {
    groups = lib.mkOption {
      default = { };
      description = "Named port groups; any listed Discord role authorizes a group.";
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            host = lib.mkOption {
              type = lib.types.str;
              default = access.firewallHost;
            };
            addressFamilies = lib.mkOption {
              type = lib.types.listOf (
                lib.types.enum [
                  4
                  6
                ]
              );
              default = [
                4
                6
              ];
            };
            label = lib.mkOption { type = lib.types.str; };
            roleIds = lib.mkOption { type = lib.types.listOf lib.types.str; };
            ports.tcp = lib.mkOption {
              type = lib.types.listOf lib.types.port;
              default = [ ];
            };
            ports.udp = lib.mkOption {
              type = lib.types.listOf lib.types.port;
              default = [ ];
            };
          };
        }
      );
    };
    frontendEnable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };
    firewallHost = lib.mkOption {
      type = lib.types.str;
      default = config.networking.hostName;
    };
    listenAddress = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Private WireGuard address for the remote broker; null uses a Unix socket.";
    };
    listenPort = lib.mkOption {
      type = lib.types.port;
      default = 9781;
    };
    tokenFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
    };
    remoteBrokers = lib.mkOption {
      default = { };
      type = lib.types.attrsOf (
        lib.types.submodule {
          options.url = lib.mkOption { type = lib.types.str; };
          options.tokenFile = lib.mkOption { type = lib.types.str; };
        }
      );
    };
    enable = lib.mkEnableOption "Discord-authorized game network access";
    hostname = lib.mkOption {
      type = lib.types.str;
      default = "punch.brage.info";
    };
    ipv4Hostname = lib.mkOption {
      type = lib.types.str;
      default = "v4.brage.info";
    };
    ipv6Hostname = lib.mkOption {
      type = lib.types.str;
      default = "v6.brage.info";
    };
    clientId = lib.mkOption {
      type = lib.types.str;
      default = "";
    };
    guildId = lib.mkOption {
      type = lib.types.str;
      default = "";
    };
    clientSecretFile = lib.mkOption {
      type = lib.types.str;
      default = "";
    };
    leaseSeconds = lib.mkOption {
      type = lib.types.ints.between 1 (14 * 24 * 60 * 60);
      default = 14 * 24 * 60 * 60;
      description = "Access duration; may be shortened for VM tests.";
    };
  };

  config = lib.mkIf access.enable {
    networking.firewall.allowedTCPPorts = lib.unique (
      lib.concatMap (g: g.ports.tcp) (builtins.attrValues localGroups)
    );
    networking.firewall.allowedUDPPorts = lib.unique (
      lib.concatMap (g: g.ports.udp) (builtins.attrValues localGroups)
    );
    assertions = [
      {
        assertion = access.listenAddress == null || access.tokenFile != null;
        message = "A TCP Punch broker requires an authentication token.";
      }
      {
        assertion = builtins.all (
          g: g.host == access.firewallHost || builtins.hasAttr g.host access.remoteBrokers
        ) (builtins.attrValues access.groups);
        message = "Every remote Punch group needs a configured broker.";
      }

      {
        assertion =
          access.groups != { }
          && builtins.all (
            name: builtins.match "[a-z][a-z0-9_]*" name != null && access.groups.${name}.roleIds != [ ]
          ) (builtins.attrNames access.groups);
        message = "Punch needs named groups with nonempty role IDs; group names use lowercase letters, digits and underscores.";
      }
      {
        assertion = config.networking.firewall.enable && !config.networking.nftables.enable;
        message = "Punch currently integrates with the iptables NixOS firewall.";
      }
      {
        assertion = builtins.all (id: builtins.match "[0-9]{1,20}" id != null) (
          (lib.optionals access.frontendEnable [
            access.clientId
            access.guildId
          ])
          ++ lib.concatMap (g: g.roleIds) (builtins.attrValues access.groups)
        );
        message = "Punch requires Discord client, guild, and role IDs.";
      }
      {
        assertion =
          builtins.all (host: builtins.match "[a-zA-Z0-9.-]+" host != null) [
            access.hostname
            access.ipv4Hostname
            access.ipv6Hostname
          ]
          &&
            builtins.length (
              lib.unique [
                access.hostname
                access.ipv4Hostname
                access.ipv6Hostname
              ]
            ) == 3;
        message = "Punch hostnames must be distinct DNS names.";
      }
    ];
    users.groups.punch = { };
    users.groups.punch-http = { };
    users.users.punch = {
      isSystemUser = true;
      group = "punch";
      extraGroups = [ "punch-http" ];
    };
    users.users.caddy = lib.mkIf access.frontendEnable { extraGroups = [ "punch-http" ]; };

    networking.firewall.extraCommands = "${broker} --restore";
    # Keep the guard across firewall stop and atomically replace it on reload.
    systemd.services.punch-firewall = {
      description = "Game address grant broker";
      wantedBy = [ "multi-user.target" ];
      requires = [ "firewall.service" ];
      after = [ "firewall.service" ];
      serviceConfig = hardening // {
        ExecStart = broker;
        Group = "punch";
        RuntimeDirectory = "punch-firewall";
        RuntimeDirectoryMode = "0750";
        StateDirectory = "punch-firewall";
        StateDirectoryMode = "0700";
        CapabilityBoundingSet = [ "CAP_NET_ADMIN" ];
        RestrictAddressFamilies = [
          "AF_UNIX"
          "AF_NETLINK"
        ]
        ++ lib.optionals (access.listenAddress != null) [
          "AF_INET"
          "AF_INET6"
        ];
        LoadCredential = lib.optional (access.tokenFile != null) "broker-token:${access.tokenFile}";
      };
    };
    systemd.services.punch = lib.mkIf access.frontendEnable {
      description = "Discord game access frontend";
      wantedBy = [ "multi-user.target" ];
      wants = [ "punch-firewall.service" ];
      after = [
        "punch-firewall.service"
        "network.target"
      ];
      environment.SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      serviceConfig = hardening // {
        ExecStart = "${python}/bin/python3 ${source}/server.py ${settings}";
        User = "punch";
        Group = "punch-http";
        SupplementaryGroups = [ "punch" ];
        RuntimeDirectory = "punch";
        RuntimeDirectoryMode = "0750";
        StateDirectory = "punch";
        StateDirectoryMode = "0700";
        LoadCredential = [
          "discord-secret:${access.clientSecretFile}"
        ]
        ++ lib.mapAttrsToList (name: b: "broker-${name}:${b.tokenFile}") access.remoteBrokers;
        CapabilityBoundingSet = "";
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
      };
    };
    services.caddy = lib.mkIf access.frontendEnable {
      enable = true;
      # Caddy's error logger also includes the request URI, which can contain
      # an OAuth code when the backend is down. Access-log suppression alone
      # does not cover that path.
      logFormat = lib.mkAfter ''
        level ERROR
        exclude http.log.error.punch_main http.log.error.punch_v4 http.log.error.punch_v6
      '';
      virtualHosts = {
        ${access.hostname} = {
          logFormat = null;
          extraConfig = ''
            log punch_main {
              output discard
            }
            ${proxy}
          '';
        };
      }
      // lib.genAttrs [ access.ipv4Hostname access.ipv6Hostname ] (host: {
        logFormat = null;
        extraConfig = ''
          log punch_${if host == access.ipv4Hostname then "v4" else "v6"} {
            output discard
          }
          handle /punch/authorize {
            ${proxy}
          }
          respond 404
        '';
      });
    };
  };
}
