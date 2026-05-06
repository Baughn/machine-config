{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "rolebot";
  version = "0.1.0";

  src = ./.;
}
