#!/usr/bin/env bash

set -eux -o pipefail

cd "$(dirname "$(readlink -f "$0")")"

if [[ ! -d /nix ]]; then
  curl https://nixos.org/nix/install | sh
  source /home/svein/.nix-profile/etc/profile.d/nix.sh
fi

nix-channel --add https://github.com/rycee/home-manager/archive/master.tar.gz home-manager
nix-channel --update
mkdir -p $HOME/.config/nixpkgs/
ln -sf $(pwd)/home/home.nix ~/.config/nixpkgs/
nix-shell '<home-manager>' -A install

