#!/usr/bin/env bash
# Check if we can reach a host via IPv6
# Exit 0 if IPv6 works, 1 if not

host="${1:-direct.brage.info}"
timeout="${2:-2}"

# Try to connect to SSH port via IPv6
if nc -6 -z -w "$timeout" "$host" 22 2>/dev/null; then
    exit 0
else
    exit 1
fi
