#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nix-output-monitor

set -euo pipefail

MODE=${1:-switch}

if [ -t 1 ]; then
  sudo nixos-rebuild $MODE --log-format internal-json |& nom --json
else
  sudo nixos-rebuild $MODE
fi
