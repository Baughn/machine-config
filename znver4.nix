# Use the Zen 4 (znver4) optimized CachyOS kernel variant.
#
# The xddxdd/nix-cachyos-kernel flake provides pre-built kernel variants
# targeting specific architectures. This selects the zen4 variant, which
# is compiled with -march=znver4 enabling AVX-512, VNNI, and other
# Zen 4 specific instructions.
#
# Only the kernel is recompiled for znver4. Userspace packages use the
# standard binary cache. For per-package znver4 optimization, add
# individual packages to the overlay below.

{ config, lib, pkgs, ... }:

{
  boot.kernelPackages = lib.mkForce pkgs.cachyosKernels.linuxPackages-cachyos-latest-zen4;
}
