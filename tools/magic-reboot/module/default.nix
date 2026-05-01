{ lib, stdenv, kernel }:

stdenv.mkDerivation {
  pname = "magic-reboot";
  version = "0.1.0";

  src = ./.;

  nativeBuildInputs = kernel.moduleBuildDependencies;

  hardeningDisable = [ "pic" ];

  # Don't use makeFlags - invoke make directly in buildPhase
  buildPhase = ''
    runHook preBuild
    make -C ${kernel.dev}/lib/modules/${kernel.modDirVersion}/build \
      M=$PWD \
      modules
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    install -D magic_reboot.ko $out/lib/modules/${kernel.modDirVersion}/extra/magic_reboot.ko
    runHook postInstall
  '';

  meta = with lib; {
    description = "Kernel module for emergency reboot via authenticated UDP packet";
    license = licenses.gpl2Only;
    platforms = platforms.linux;
  };
}
