#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

jj squash \
  --from 'reachable(latest(Bumps), Bumps)' \
  --to 'latest(Bumps)' \
  --use-destination-message \
  || true
jj bookmark set master -r 'latest(ancestors(@-) & ~empty() & ~description(exact:""))'
jj git push
