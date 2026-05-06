{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "aniwatch";
  version = "0.1.0";

  src = ./.;
}
