#!/usr/bin/env bash

tools-for-claude/format-nix.sh || exit 2
if command -v colmena > /dev/null; then
  colmena build || exit 2
fi
