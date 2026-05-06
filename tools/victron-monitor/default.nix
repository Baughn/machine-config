{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "victron-monitor";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
