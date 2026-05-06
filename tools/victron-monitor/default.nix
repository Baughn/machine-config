{ pkgs, ... }:

pkgs.mkCranePackage {
  pname = "victron-monitor";
  version = "0.1.0";

  src = ./.;
  extraFiles = [ ./example.json ];
}
