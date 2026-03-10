#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nix-output-monitor
set -euo pipefail

sudo nixos-rebuild switch --log-format internal-json "$@" |& nom --json
