#!/usr/bin/env nix-shell
#!nix-shell -i bash -p bash disnix

set -eu

cd "$(readlink -f "$(dirname "$0")")"

disnix-env -s services/services.nix -i production/infrastructure.nix -d production/distribution.nix  "$@"
