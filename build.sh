#!/usr/bin/env zsh

set -xue -o pipefail
cd "$(dirname "$(readlink -f "$0")")"

export HERE="$(pwd)"
export CHANNEL="18.03"

update() {
   BASE=$(curl https://howoldis.herokuapp.com/api/channels | \
              jq -r "map(select(.name == \"nixos-$CHANNEL\"))[0].commit")
   if [[ $BASE = "null" ]]; then
     BASE="origin/release-$CHANNEL"
     pushd $HOME/dev/nix-system; git fetch; popd
   fi
   echo "Building nix-system tree from version $BASE..."
   export NIXPKGS="$HOME/dev/nix-system"
   pushd "$NIXPKGS"
     git reset --hard; git clean -fxd
     git cat-file -t $BASE 2>/dev/null >/dev/null || git fetch origin
     git checkout $BASE
     git branch -D system
     git branch system
     cat "$HERE/CHERRY_PICKS" | while read pick; do
         if ! [[ $pick =~ '^#' ]]; then
           git cherry-pick $pick
         fi
     done
   popd
}

{
  update
  nix build '(import ./machines.nix).all'

  nixops modify -d personal ./personal.nix
  nixops deploy -d personal --check -j 8 --cores 16 -I "nixpkgs=$HOME/dev/nix-system" "$@"
}
