{stdenv}:

# Empty: Mediawiki does all its own initialization.
#
# Otherwise, if we included any .sql files in here they'd be run on first
# activation.
stdenv.mkDerivation {
  name = "MediawikiDb";
  src = ../../third_party/mediawiki-1.30.0;
  installPhase = ''
    mkdir $out
    touch $out/.ignore
  '';
}
