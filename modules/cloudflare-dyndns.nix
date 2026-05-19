{ config, lib, pkgs, ... }:

let
  cfg = config.me.cloudflareDyndns;

  script = pkgs.writeShellApplication {
    name = "cloudflare-dyndns";
    runtimeInputs = with pkgs; [ iproute2 curl jq ];
    text = ''
      set -euo pipefail

      : "''${HOSTNAME_FQDN:?HOSTNAME_FQDN must be set}"
      : "''${ZONE_NAME:?ZONE_NAME must be set}"

      token=$(< "''${CREDENTIALS_DIRECTORY}/token")

      iface_filter=()
      if [[ -n "''${INTERFACE:-}" ]]; then
        iface_filter=(dev "$INTERFACE")
      fi

      # Stable public IPv6: scope global, not temporary (RFC 4941),
      # not deprecated, not ULA (fc00::/7).
      addr=$(ip -6 -json addr show scope global "''${iface_filter[@]}" \
        | jq -r '
            [ .[].addr_info[]?
              | select(.scope == "global")
              | select((.temporary // false) | not)
              | select((.deprecated // false) | not)
              | select((.local | startswith("fc")) or (.local | startswith("fd")) | not)
              | .local
            ][0] // ""')

      if [[ -z "$addr" ]]; then
        echo "no global IPv6 address available; skipping"
        exit 0
      fi

      api="https://api.cloudflare.com/client/v4"
      auth_header="Authorization: Bearer $token"
      type_header="Content-Type: application/json"

      zone_id=$(curl --fail-with-body --silent --show-error \
        -H "$auth_header" -H "$type_header" \
        "$api/zones?name=$ZONE_NAME" | jq -r '.result[0].id // empty')
      if [[ -z "$zone_id" ]]; then
        echo "zone $ZONE_NAME not found" >&2
        exit 1
      fi

      record=$(curl --fail-with-body --silent --show-error \
        -H "$auth_header" -H "$type_header" \
        "$api/zones/$zone_id/dns_records?type=AAAA&name=$HOSTNAME_FQDN")
      record_id=$(jq -r '.result[0].id // empty' <<<"$record")
      record_content=$(jq -r '.result[0].content // empty' <<<"$record")

      payload=$(jq -nc \
        --arg name "$HOSTNAME_FQDN" \
        --arg content "$addr" \
        '{type:"AAAA", name:$name, content:$content, ttl:120, proxied:false}')

      if [[ -z "$record_id" ]]; then
        echo "creating AAAA $HOSTNAME_FQDN -> $addr"
        resp=$(curl --fail-with-body --silent --show-error \
          -H "$auth_header" -H "$type_header" \
          -X POST --data "$payload" \
          "$api/zones/$zone_id/dns_records")
      elif [[ "$record_content" == "$addr" ]]; then
        echo "AAAA $HOSTNAME_FQDN already $addr; unchanged"
        exit 0
      else
        echo "updating AAAA $HOSTNAME_FQDN: $record_content -> $addr"
        resp=$(curl --fail-with-body --silent --show-error \
          -H "$auth_header" -H "$type_header" \
          -X PATCH --data "$payload" \
          "$api/zones/$zone_id/dns_records/$record_id")
      fi

      if [[ "$(jq -r '.success' <<<"$resp")" != "true" ]]; then
        echo "Cloudflare API error: $resp" >&2
        exit 1
      fi
      echo "ok"
    '';
  };
in
{
  options.me.cloudflareDyndns = {
    enable = lib.mkEnableOption "Cloudflare IPv6 dynamic DNS updater";

    hostname = lib.mkOption {
      type = lib.types.str;
      description = "FQDN of the AAAA record to keep current (e.g. \"saya.brage.info\").";
    };

    zone = lib.mkOption {
      type = lib.types.str;
      description = "Cloudflare zone the record belongs to (e.g. \"brage.info\").";
    };

    tokenFile = lib.mkOption {
      type = lib.types.path;
      description = ''
        Path to a file holding a Cloudflare API token with Zone:DNS:Edit
        on the configured zone. Typically
        config.age.secrets.cloudflare-dyndns-token.path.
      '';
    };

    interface = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = ''
        Optional interface to scan for the public IPv6. When null, every
        interface is considered and the first non-temporary, non-deprecated,
        non-ULA global address wins.
      '';
    };

    interval = lib.mkOption {
      type = lib.types.str;
      default = "5min";
      description = "systemd OnUnitActiveSec for the update timer.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.cloudflare-dyndns = {
      description = "Update Cloudflare AAAA record for ${cfg.hostname}";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment = {
        HOSTNAME_FQDN = cfg.hostname;
        ZONE_NAME = cfg.zone;
      } // lib.optionalAttrs (cfg.interface != null) {
        INTERFACE = cfg.interface;
      };
      serviceConfig = {
        Type = "oneshot";
        ExecStart = lib.getExe script;
        LoadCredential = "token:${cfg.tokenFile}";
        DynamicUser = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_NETLINK" ];
      };
    };

    systemd.timers.cloudflare-dyndns = {
      description = "Periodic Cloudflare AAAA refresh for ${cfg.hostname}";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "1min";
        OnUnitActiveSec = cfg.interval;
        Persistent = true;
        Unit = "cloudflare-dyndns.service";
      };
    };
  };
}
