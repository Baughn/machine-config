{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "easel";
  version = "0.1.0";

  src = ./.;
  # The `easel ops` reference text is compiled in with include_str!.
  extraFiles = [ ./src/help ];
}
