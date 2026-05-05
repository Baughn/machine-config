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

{ lib, pkgs, ... }:

let
  baseKernelPackages = pkgs.cachyosKernels.linuxPackages-cachyos-latest-zen4;
  kernel = baseKernelPackages.kernel.override {
    structuredExtraConfig = baseKernelPackages.kernel.structuredExtraConfig // (with lib.kernel; {
      # Compile out AF_ALG, the userspace socket API for the kernel crypto
      # subsystem. CVE-2026-31431 is in algif_aead; the rest of the family is
      # disabled with it because nothing on saya depends on AF_ALG.
      CRYPTO_USER_API = lib.mkForce no;
      CRYPTO_USER_API_AEAD = lib.mkForce no;
      CRYPTO_USER_API_HASH = lib.mkForce no;
      CRYPTO_USER_API_RNG = lib.mkForce no;
      CRYPTO_USER_API_SKCIPHER = lib.mkForce no;
    });
  };
in
{
  boot.kernelPackages = lib.mkForce (pkgs.linuxPackagesFor kernel);
}
