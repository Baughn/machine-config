{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "irc-tool";
  version = "0.1.0";

  src = ./.;
}
