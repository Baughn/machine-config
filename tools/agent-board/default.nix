{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "agent-board";
  version = "0.1.0";

  src = ./.;
}
