{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "aniwatch";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
