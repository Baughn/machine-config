#!/usr/bin/env bash

set -euo pipefail

# Build kexec
nix build .\#packages.x86_64-linux.install-kexec -o install-kexec
# The tarball needs to be unpacked to a writable location
TMPDIR=$(mktemp -d)
tar -xJf ./install-kexec/tarball/*.tar.xz -C $TMPDIR
# Run kexec.
sudo $TMPDIR/kexec_nixos
