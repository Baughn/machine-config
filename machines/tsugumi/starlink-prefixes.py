"""Maintain the Victron ingress allowlist from RIPE RIS announcements."""

import argparse
from datetime import datetime, timedelta, timezone
import ipaddress
import json
from pathlib import Path
import subprocess
import urllib.parse
import urllib.request

CACHE = Path("/var/lib/victron-prefixes/prefixes.json")
MAX_RESPONSE_BYTES = 4 * 1024 * 1024


def validate_prefixes(prefixes):
    if not isinstance(prefixes, list) or not 1 <= len(prefixes) <= 20000:
        raise ValueError("Missing or excessive prefix list")
    networks = [ipaddress.ip_network(prefix) for prefix in prefixes]
    if any(not net.is_global or net.prefixlen < (8 if net.version == 4 else 16)
           for net in networks):
        raise ValueError("Non-public or unexpectedly broad prefix")
    # Require both address families; incomplete API responses must not replace
    # a working cache. Collapse overlapping BGP announcements before installing.
    families = [[net for net in networks if net.version == version] for version in (4, 6)]
    if not all(families):
        raise ValueError("Missing IPv4 or IPv6 prefixes")
    return [str(net) for family in families for net in ipaddress.collapse_addresses(family)]


def fetch_prefixes():
    # A short window avoids keeping routes withdrawn weeks ago. This is an
    # origin-AS allowlist, not an authentication mechanism or an RPKI verifier.
    start = datetime.now(timezone.utc) - timedelta(hours=6)
    query = urllib.parse.urlencode({"resource": "AS14593", "starttime": int(start.timestamp())})
    url = "https://stat.ripe.net/data/announced-prefixes/data.json?" + query
    with urllib.request.urlopen(url, timeout=30) as response:
        body = response.read(MAX_RESPONSE_BYTES + 1)
    if len(body) > MAX_RESPONSE_BYTES:
        raise ValueError("RIPE response exceeded size limit")
    result = json.loads(body)
    if result.get("status") != "ok" or str(result["data"]["resource"]).removeprefix("AS") != "14593":
        raise ValueError("Unexpected RIPE response")
    return validate_prefixes([item["prefix"] for item in result["data"]["prefixes"]])


def ruleset(prefixes):
    families = {4: [], 6: []}
    for prefix in prefixes:
        families[ipaddress.ip_network(prefix).version].append(prefix)
    sets = []
    for version, address_type in [(4, "ipv4_addr"), (6, "ipv6_addr")]:
        elements = ", ".join(families[version])
        sets.append(f"set starlink{version} {{ type {address_type}; flags interval; "
                    + (f"elements = {{ {elements} }}; " if elements else "") + "}")
    return "\n".join([
        "destroy table inet victron_ingress",
        "table inet victron_ingress {",
        *sets,
        "chain input {",
        "type filter hook input priority -10; policy accept;",
        "meta nfproto ipv4 udp dport 9099 ip saddr != @starlink4 drop",
        "meta nfproto ipv6 udp dport 9099 ip6 saddr != @starlink6 drop",
        "}", "}",
    ]) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--restore", action="store_true", help="Restore cache without network access")
    args = parser.parse_args()
    if args.restore:
        try:
            prefixes = validate_prefixes(json.loads(CACHE.read_text()))
        except (OSError, ValueError):
            prefixes = []  # First boot/corrupt cache: deny all until a successful refresh.
    else:
        prefixes = fetch_prefixes()
    # nft commits the entire transaction atomically, including on first boot.
    subprocess.run(["@nft@", "-f", "-"], input=ruleset(prefixes), text=True, check=True)
    if not args.restore:
        temporary = CACHE.with_suffix(".tmp")
        temporary.write_text(json.dumps(prefixes) + "\n")
        temporary.replace(CACHE)
    print(f"Installed {len(prefixes)} Starlink prefixes for Victron UDP/9099")


if __name__ == "__main__":
    main()
