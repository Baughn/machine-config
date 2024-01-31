{ pkgs, ... }:

let
  # built --with github.com/greenpau/caddy-security
  securedCaddy = pkgs.stdenv.mkDerivation {
    name = "caddy-${pkgs.caddy.version}-secured";
    buildInputs = [ pkgs.xcaddy pkgs.go pkgs.libcap pkgs.xorg.lndir ];
    dontUnpack = true;
    buildPhase = ''
      HOME=$TMPDIR
      xcaddy build --with github.com/greenpau/caddy-security
      mkdir $out
      lndir ${pkgs.caddy} $out/
      rm $out/bin/caddy
      cp -a caddy $out/bin/caddy
    '';
    # Disable sandboxing, because it breaks the build
    __noChroot = true;
  };
in
{
  services.caddy.package = securedCaddy;
}
