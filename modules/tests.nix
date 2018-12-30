{ config, pkgs, ... }:

let
  test = file: (import (pkgs.path + "/nixos/tests/${file}.nix") {
    inherit config;
  });

  xmonad = test "xmonad";

  tests = pkgs.runCommand "proof-of-tests" {
    tests = [ xmonad ];
  } ''
    mkdir -p $out/share/doc/proof-of-tests
    i=1
    for test in $tests; do
      ln -s $test $out/share/doc/proof-of-tests/$i
      i=$(($i + 1))
    done
    '';
in {
  environment.systemPackages = [
    tests
  ];
}
