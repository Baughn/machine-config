#!/usr/bin/env bash

set -eu -o pipefail
cd "$(readlink -f "$(dirname "$0")")"
set -x

HOST="${1:-}"
if [[ ! -z "$HOST" ]]; then
  shift
fi
nix run github:serokell/deploy-rs "path:.#$HOST" "$@"
