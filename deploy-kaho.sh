#!/usr/bin/env bash
# Local deploy for kaho (nix-darwin). Run on kaho itself.

set -euo pipefail

MODE=${1:-switch}

sudo darwin-rebuild "$MODE" --flake .#kaho
