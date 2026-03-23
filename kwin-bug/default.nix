{ stdenv, linuxHeaders }:

stdenv.mkDerivation {
  pname = "drm-atomic-log";
  version = "0.1.0";
  src = ./.;

  buildInputs = [ linuxHeaders ];

  buildPhase = ''
    gcc -shared -fPIC -o drm-atomic-log.so drm-atomic-log.c \
      -ldl -I${linuxHeaders}/include -Wall -Wextra -O2
  '';

  installPhase = ''
    mkdir -p $out/lib
    cp drm-atomic-log.so $out/lib/
  '';
}
