#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nix-output-monitor

set -euo pipefail

MODE=${1:-switch}

if [ -t 1 ]; then
  nix build .#all-systems --log-format internal-json |& nom --json
else
  nix build .#all-systems
fi

colmena apply --on tsugumi "$MODE"
