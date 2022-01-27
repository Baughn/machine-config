#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nvd

set -exo pipefail

cd "$(dirname "$(readlink -f "$0")")"

OLDLOCK=$(mktemp)
trap "rm $OLDLOCK" EXIT
cat flake.lock > $OLDLOCK
nix flake update
if nix flake check; then
  nixos-rebuild --flake . build
  nvd diff /run/current-system result
else
  cat $OLDLOCK > flake.lock
fi
