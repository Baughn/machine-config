{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "rolebot";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
