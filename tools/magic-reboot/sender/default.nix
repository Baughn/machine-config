{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "magic-reboot-send";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  meta = with pkgs.lib; {
    description = "Send magic packet to trigger emergency reboot on remote machine";
    license = licenses.mit;
    platforms = platforms.unix;
  };
}
