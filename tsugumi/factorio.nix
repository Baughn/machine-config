{ config, pkgs, ... }:

let
  version = "1.1.59";
in

{
  services.factorio = {
    enable = true;
    admins = ["Baughn"];
    description = "Erisia";
    game-name = "Erisia";
    game-password = builtins.readFile ../secrets/factorio.pw;
    nonBlockingSaving = true;
    openFirewall = true;
    loadLatestSave = true;
    saveName = "terracognito";
    package = pkgs.factorio-headless.overrideAttrs (_: {
      inherit version;
      src = pkgs.fetchurl {
        url = "https://factorio.com/get-download/${version}/headless/linux64";
        name = "factorio-headless-${version}.tar.xz";
        sha256 = "sha256-r5ECvusV4HnwwJUn0jM6F/j4nk9cSvzPtdt2buL0vNw=";
      };
    });
  };
}
