#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

jj git fetch
if command -v ping-discord > /dev/null; then
  ping-discord update -m 'Pulled changes:' -p '*' -r 'master..master@origin'
else
  echo 'No ping-discord here (skipped)'
fi
jj rebase -r master..@ -d master@origin
