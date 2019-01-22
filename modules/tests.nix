{ config, pkgs, ... }:

let
  file = f: (import (pkgs.path + "/nixos/tests/${f}.nix"));

  test = f: (file f {
    inherit config;
  });

  xmonad = test "xmonad";
  gnome = test "gnome3-gdm";
  # The 19.03 version should work with just `(test "zfs").stable`.
  zfs = (file "zfs" {}).stable {};

  tests = pkgs.runCommand "proof-of-tests" {
    tests = [ xmonad gnome zfs ];
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
