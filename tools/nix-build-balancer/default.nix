{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "nix-build-balancer";
  version = "0.2.0";

  src = ./.;
  extraFiles = [ ./src/persistence/schema.sql ];
}
