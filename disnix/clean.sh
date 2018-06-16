#!/usr/bin/env nix-shell
#!nix-shell -i bash -p bash disnix

set -eu

cd "$(readlink -f "$(dirname "$0")")"

disnix-collect-garbage -d production/infrastructure.nix
