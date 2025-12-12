#!/usr/bin/env bash

tools-for-claude/format-nix.sh || exit 2
if command -v nix-check-cached >/dev/null; then
  nix-check-cached || exit 2
fi
