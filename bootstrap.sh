#!/usr/bin/env nix-shell
#!nix-shell -i bash -p git jujutsu

nix run github:zhaofengli/colmena --extra-experimental-features 'nix-command flakes' -- apply-local --sudo boot
