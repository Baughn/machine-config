{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "magic-reboot-send";
  version = "0.1.0";

  src = ./.;

  meta = with pkgs.lib; {
    description = "Send magic packet to trigger emergency reboot on remote machine";
    license = licenses.mit;
    platforms = platforms.unix;
  };
}
