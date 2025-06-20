#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

jj git fetch
jj rebase -r shiny..@ -d shiny@origin
