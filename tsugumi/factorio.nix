{ config, pkgs, ... }:

let
  version = "1.1.57";
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
    saveName = "terracognito";
    package = pkgs.factorio-headless.overrideAttrs (_: {
      inherit version;
      src = pkgs.fetchurl {
        url = "https://factorio.com/get-download/${version}/headless/linux64";
        name = "factorio-headless-${version}.tar.xz";
        sha256 = "sha256-tWHdy+T2mj5WURHfFmALB+vUskat7Wmeaeq67+7lxfg=";
      };
    });
  };
}
