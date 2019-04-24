#!/usr/bin/env bash

set -eux

nix build \
  -f '<nixpkgs/nixos>' \
  config.system.build.isoImage \
  -I nixos-config=./cd.nix \
  -o result.cd

nix build \
  -f '<nixpkgs/nixos>' \
  config.system.build.kexec_tarball \
  -I nixos-config=./kexec.nix \
  -o result.kexec_tarball

nix build \
  -f '<nixpkgs/nixos>' \
  config.system.build.kexec_script \
  -I nixos-config=./kexec.nix \
  -o result.kexec_script
