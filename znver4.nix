# Compile all packages targeting AMD Zen 4 (znver4) architecture.
#
# This sets -march=znver4 on every package build via the cc-wrapper, enabling
# AVX-512, VNNI, and other Zen 4 specific instructions. Combined with the
# CachyOS kernel, this replicates the full CachyOS x86-64-v4/znver4 repo
# optimization strategy.
#
# WARNING: This means nearly all packages will be compiled from source.
# No binary cache will have znver4-optimized builds. Initial builds will
# take a very long time, but subsequent rebuilds only recompile changed
# packages. Consider using `nix.settings.max-jobs` and `nix.settings.cores`
# to maximize build parallelism on your 7950X3D.

{ config, lib, pkgs, ... }:

{
  nixpkgs.hostPlatform = {
    system = "x86_64-linux";
    gcc.arch = "znver4";
    gcc.tune = "znver4";
  };
}
