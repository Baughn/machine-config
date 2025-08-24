#!/usr/bin/env bash

tools-for-claude/format-nix.sh || exit 2
nix-check-cached || exit 2
