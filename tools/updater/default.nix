{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "nixos-updater";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
