{ config, lib, pkgs, ... }:

# gpu-unlock (https://github.com/Zagitta/gpu-unlock, ada-beta branch):
# a kernel module that, before nvidia binds the card, uses NVIDIA's signed
# SEC2 Booter to lift the consumer FP16/BF16-into-FP32 tensor rate limit,
# then FLRs the card. The unlock is volatile (lost on power off; the module
# redoes it on resume from suspend).
#
# The module is inert unless gpu_unlock.targets= is on the kernel command
# line. It loads from the initrd; nvidia isn't in saya's initrd, so it
# always runs before nvidia can bind the 4090.
#
# Check the result with: journalctl -k -b | grep gpu_unlock
let
  kernel = config.boot.kernelPackages.kernel;

  src = pkgs.fetchFromGitHub {
    owner = "Zagitta";
    repo = "gpu-unlock";
    rev = "297835705977911065d69da3c504d5bd2be0faaf";
    hash = "sha256-4TK0Xq/dZ4ogB5NuxQaSivxk5Ka/qCGkJdVLCeIZeqo=";
  };
  version = "1.1.0-ada-beta";

  # The firmware is extracted from this exact driver release, whatever
  # driver saya runs; the extractors refuse any other version.
  nv = "610.43.03";
  nvSource = pkgs.fetchurl {
    url = "https://download.nvidia.com/XFree86/NVIDIA-kernel-module-source/NVIDIA-kernel-module-source-${nv}.tar.xz";
    sha256 = "7e118923c7a23edc36114d63273a46e3e04e9af98695a42203e7ac2dfe9fc1dc";
  };
  nvInstaller = pkgs.fetchurl {
    url = "https://download.nvidia.com/XFree86/Linux-x86_64/${nv}/NVIDIA-Linux-x86_64-${nv}-no-compat32.run";
    sha256 = "e084451db730a6df29d3d74f655749c4d236e3c945f7d94b5205e84318fe2f90";
  };

  firmware = pkgs.stdenvNoCC.mkDerivation {
    pname = "gpu-unlock-firmware";
    inherit version src;
    nativeBuildInputs = [ pkgs.python3 pkgs.zstd ];
    dontConfigure = true;
    buildPhase = ''
      runHook preBuild
      mkdir nv
      tar -xf ${nvSource} -C nv
      python3 firmware/extract_gsp.py ${nvInstaller} assets
      python3 firmware/extract_boot_assets.py nv/NVIDIA-kernel-module-source-${nv} assets
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      make install-firmware ASSETS=assets FWDIR=$out/lib/firmware/gpu-unlock/${nv} \
        LICENSEDIR=$out/share/licenses
      runHook postInstall
    '';
    meta.license = lib.licenses.unfree;
  };

  module = pkgs.stdenv.mkDerivation {
    pname = "gpu-unlock";
    version = "${version}-${kernel.version}";
    inherit src;
    sourceRoot = "${src.name}/kernel/gpu_unlock";
    nativeBuildInputs = kernel.moduleBuildDependencies;
    makeFlags = kernel.commonMakeFlags ++ [
      "KDIR=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
    ];
    installPhase = ''
      runHook preInstall
      install -Dm444 gpu_unlock.ko -t $out/lib/modules/${kernel.modDirVersion}/extra
      runHook postInstall
    '';
    meta.license = lib.licenses.gpl2Only;
  };
in
{
  boot.extraModulePackages = [ module ];
  # The initrd picks up the firmware through the module's MODULE_FIRMWARE list.
  boot.initrd.kernelModules = [ "gpu_unlock" ];
  hardware.firmware = [ firmware ];

  # Opt-in: the 4090 (lspci -D). Remove it, or delete it in the systemd-boot
  # edit screen for one boot, to boot without the unlock.
  boot.kernelParams = [ "gpu_unlock.targets=all" ];
}
