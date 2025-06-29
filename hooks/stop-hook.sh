#!/usr/bin/env bash

tools-for-claude/format-nix.sh || exit 2
colmena build || exit 2
