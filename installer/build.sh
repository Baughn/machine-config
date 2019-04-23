nix build \
  -f ~/dev/nix/system/nixos \
  config.system.build.isoImage \
  -I nixos-config=./cd.nix \
  -o result.cd

nix build \
  -f ~/dev/nix/system/nixos \
  config.system.build.kexec_tarball \
  -I nixos-config=./kexec.nix \
  -o result.kexec

