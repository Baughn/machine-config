#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

jj git fetch
ping-discord update -m 'Pulled changes:' -p '*' -r 'master..master@origin'
jj rebase -r master..@ -d master@origin
