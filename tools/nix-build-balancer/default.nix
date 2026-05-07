{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "nix-build-balancer";
  version = "0.1.0";

  src = ./.;
}
