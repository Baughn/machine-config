{stdenv, xorg}:
{WikiDb}:

# Take the base mediawiki tarball, then overlay it with site configuration.

stdenv.mkDerivation rec {
  name = "Mediawiki";
  src = ../../third_party/mediawiki-1.30.0;
  data = ./data;
  installPhase = ''
    mkdir -p $out/webapps/mediawiki
    ${xorg.lndir}/bin/lndir $src $out/webapps/mediawiki
    ${xorg.lndir}/bin/lndir $data $out/webapps/mediawiki
  '';
}
