#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nix-output-monitor

set -euo pipefail

MODE=${1:-switch}

if [ -t 1 ]; then
  nixos-rebuild $MODE --flake .#v4 --sudo --target-host v4.brage.info --log-format internal-json |& nom --json
else
  nixos-rebuild $MODE --flake .#v4 --sudo --target-host v4.brage.info
fi
