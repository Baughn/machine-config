{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "ping-discord";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
