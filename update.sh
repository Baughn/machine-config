#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nvd

set -eo pipefail
set -x

cd "$(dirname "$(readlink -f "$0")")"

OLDLOCK=$(mktemp)
trap "rm $OLDLOCK" EXIT
cat flake.lock > $OLDLOCK
nix --extra-experimental-features 'nix-command flakes' flake update
nix flake check
if nixos-rebuild --flake . build --show-trace "$@"; then
  nvd diff /run/current-system result
  printf '\a'
  PS3='Deploy? '
  select opt in exit switch boot; do
    case $opt in
      exit)
        break
        ;;
      switch)
        sudo nixos-rebuild --flake . switch
        break
        ;;
      boot)
        sudo nixos-rebuild --flake . boot
        break
        ;;
    esac
  done
else
  printf '\a'
  cat $OLDLOCK > flake.lock
fi
