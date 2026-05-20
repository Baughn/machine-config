{ config, lib, pkgs, ... }:

let
  cfg = config.me.cloudflareDyndns;

  script = pkgs.writers.writePython3Bin "cloudflare-dyndns" { doCheck = false; } ''
    """Update a Cloudflare AAAA record with this host's stable public IPv6."""

    import json
    import os
    import subprocess
    import sys
    import urllib.error
    import urllib.request

    API = "https://api.cloudflare.com/client/v4"


    def log(msg):
        print(msg, flush=True)


    def die(msg, code=1):
        print(msg, file=sys.stderr, flush=True)
        sys.exit(code)


    def pick_ipv6(interface):
        cmd = ["ip", "-6", "-json", "addr", "show", "scope", "global"]
        if interface:
            cmd += ["dev", interface]
        out = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
        for link in json.loads(out):
            for a in link.get("addr_info", []):
                if a.get("scope") != "global":
                    continue
                if a.get("temporary") or a.get("deprecated"):
                    continue
                local = a.get("local", "")
                # Skip ULA (fc00::/7).
                if local.startswith(("fc", "fd")):
                    continue
                return local
        return None


    def api(method, url, token, body=None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(
            url,
            method=method,
            data=data,
            headers={
                "Authorization": f"Bearer {token}",
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(req) as resp:
                return json.loads(resp.read())
        except urllib.error.HTTPError as e:
            die(f"{method} {url}: HTTP {e.code}: {e.read().decode(errors='replace')}")


    def main():
        hostname = os.environ["HOSTNAME_FQDN"]
        zone_name = os.environ["ZONE_NAME"]
        interface = os.environ.get("INTERFACE") or None
        state_dir = os.environ["STATE_DIRECTORY"]
        cache_path = os.path.join(state_dir, "last.json")

        addr = pick_ipv6(interface)
        if not addr:
            log("no global IPv6 address available; skipping")
            return

        # Skip the Cloudflare round-trip if we already pushed this exact
        # (hostname, addr) and the last run succeeded.
        try:
            with open(cache_path) as f:
                cached = json.load(f)
            if cached.get("hostname") == hostname and cached.get("addr") == addr:
                log(f"AAAA {hostname} already {addr} (cached); skipping API")
                return
        except (FileNotFoundError, json.JSONDecodeError):
            pass

        with open(os.path.join(os.environ["CREDENTIALS_DIRECTORY"], "token")) as f:
            token = f.read().strip()

        zones = api("GET", f"{API}/zones?name={zone_name}", token)
        if not zones["result"]:
            die(f"zone {zone_name} not found")
        zone_id = zones["result"][0]["id"]

        records = api(
            "GET",
            f"{API}/zones/{zone_id}/dns_records?type=AAAA&name={hostname}",
            token,
        )
        existing = records["result"][0] if records["result"] else None

        # ttl: 1 is Cloudflare's sentinel for "auto".
        body = {
            "type": "AAAA",
            "name": hostname,
            "content": addr,
            "ttl": 1,
            "proxied": False,
        }

        if existing is None:
            log(f"creating AAAA {hostname} -> {addr}")
            resp = api("POST", f"{API}/zones/{zone_id}/dns_records", token, body)
        elif existing["content"] == addr:
            log(f"AAAA {hostname} already {addr}; unchanged")
            resp = None
        else:
            log(f"updating AAAA {hostname}: {existing['content']} -> {addr}")
            resp = api(
                "PATCH",
                f"{API}/zones/{zone_id}/dns_records/{existing['id']}",
                token,
                body,
            )

        if resp is not None and not resp.get("success"):
            die(f"Cloudflare API error: {resp}")

        tmp = cache_path + ".tmp"
        with open(tmp, "w") as f:
            json.dump({"hostname": hostname, "addr": addr}, f)
        os.replace(tmp, cache_path)
        log("ok")


    if __name__ == "__main__":
        main()
  '';
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
      path = [ pkgs.iproute2 ];
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
        StateDirectory = "cloudflare-dyndns";
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
