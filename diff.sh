#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nix-diff

set -exu -o pipefail
cd "$(dirname "$(readlink -f "$0")")"

MACHINE="${1:-saya}"

EXP="$(pwd)"
BASE="$(mktemp -d)"
trap "rm -rf $BASE" EXIT

cd "$BASE"
git clone file://"$EXP" --depth 1
cd nixos
ln -s "$EXP"/secrets .
BASEDRV="$(nix-instantiate '<nixpkgs/nixos>' -I nixos-config=$MACHINE/configuration.nix | head -n1)"
cd "$EXP"
EXPDRV="$(nix-instantiate '<nixpkgs/nixos>' -I nixos-config=$MACHINE/configuration.nix | head -n1)"

nix-diff "$BASEDRV" "$EXPDRV" | less -R
