with import <nixpkgs> {};

let
  nixos = import <nixpkgs/nixos>;
  cd = nixos { configuration = ./cd.nix; };
  kexec = nixos { configuration = ./kexec.nix; };
in
  linkFarm "installers" [
    { name = "cd"; path = cd.config.system.build.isoImage; }
    { name = "kexec_tarball"; path = kexec.config.system.build.kexec_tarball; }
    { name = "kexec_script"; path = kexec.config.system.build.kexec_script; }
  ]

