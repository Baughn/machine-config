{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "irc-tool";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
