#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

# Check for minecraft-related changes
ping-discord update --patterns "*minecraft*,*ssh_host_*,modules/users.nix,modules/keys.nix,push.sh,tools/ping-discord/*" || true

jj bookmark set master -r 'latest(ancestors(@-) & ~empty() & ~description(exact:""))'
jj git push
