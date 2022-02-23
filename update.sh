#!/usr/bin/env nix-shell
#!nix-shell -i bash -p nvd

set -eo pipefail

cd "$(dirname "$(readlink -f "$0")")"

OLDLOCK=$(mktemp)
trap "rm $OLDLOCK" EXIT
cat flake.lock > $OLDLOCK
nix flake update
if cmp -s flake.lock $OLDLOCK; then
  echo 'Nothing changed'
  exit 0
fi
if nix flake check; then
  nixos-rebuild --flake . build
  nvd diff /run/current-system result
  PS3='Deploy? '
  select opt in exit switch boot; do
    case $opt in
      exit)
        break
        ;;
      switch)
        sudo result/bin/switch-to-configuration switch
        break
        ;;
      boot)
        sudo result/bin/switch-to-configuration boot
        break
        ;;
    esac
  done
else
  cat $OLDLOCK > flake.lock
fi
