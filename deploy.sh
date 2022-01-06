#!/usr/bin/env nix-shell
#!nix-shell -i bash -p

set -eu -o pipefail
cd "$(dirname "$(readlink -f "$0")")"

HOST="${1:-}"
if [[ ! -z "$HOST" ]]; then
  shift
fi
set -x
nix run github:serokell/deploy-rs "path:.#$HOST" "$@"
