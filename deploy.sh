#!/usr/bin/env nix-shell
#!nix-shell -i bash -p

set -eu -o pipefail
cd "$(dirname "$(readlink -f "$0")")"
set -x

HOST="${1:-}"
if [[ ! -z "$HOST" ]]; then
  shift
fi
nix run github:serokell/deploy-rs "path:.#$HOST" "$@"
