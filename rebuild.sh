#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nix-output-monitor

set -euo pipefail

MODE=${1:-switch}

sudo nixos-rebuild $MODE --log-format internal-json |& nom --json
