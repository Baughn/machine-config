#!/usr/bin/env zsh

set -xue -o pipefail
cd "$(dirname "$(readlink -f "$0")")"

HERE="$(pwd)"

update() {
  sudo nix-channel --update

  if [[ -e CHERRY_PICKS ]]; then
      BASE=$(nix-instantiate --eval -E '(import <nixpkgs> {}).lib.nixpkgsVersion' | \
                 sed 's/.*\.//; s/"//')
      echo "Building nix-system tree from version $BASE..."
      export NIXPKGS="$HOME/dev/nix-system"
      pushd "$NIXPKGS"
        git reset --hard; git clean -fxd
        git cat-file -t $BASE 2>/dev/null >/dev/null || git fetch origin
        git checkout $BASE
        git branch -D system
        git branch system
        cat "$HERE/CHERRY_PICKS" | while read pick; do
            git cherry-pick $pick
        done
      popd
  fi
}

{
  if [[ $(find .timestamp -mtime +3 2>&1 | wc -l) -gt 0 ]]; then
      update
      touch .timestamp
  fi

  nixops modify -d personal ./personal.nix
  nixops deploy -d personal --check -j 8 --cores 16 -I "nixpkgs=$HOME/dev/nix-system" "$@"
}
