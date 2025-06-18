#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

jj bookmark set shiny -r 'latest(ancestors(@) & ~empty() & ~description(exact:""))'
jj git push
