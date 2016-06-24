#!/usr/bin/env nix-shell
#!nix-shell -p stow -i bash

find . -mindepth 1 -maxdepth 1 -type d -not -name .git \
  -printf '%f\0' \
  | xargs -0 stow
