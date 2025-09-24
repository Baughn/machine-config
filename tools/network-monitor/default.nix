{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "network-monitor";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
