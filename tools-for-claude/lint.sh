#!/usr/bin/env nix-shell
#!nix-shell -i bash -p statix deadnix

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"/..

tools-for-claude/format-nix.sh
statix check
deadnix
