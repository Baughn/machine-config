#!/usr/bin/env nix-shell
#!nix-shell -i zsh -p zsh

set -ue -o pipefail
cd "$(dirname "$(readlink -f "$0")")"

export HERE="$(pwd)"
export CHANNEL="unstable"
export NIXPKGS="$HOME/dev/nix/system"
export FORCE_UPDATE=0

get_base() {
  BASE="$(curl 'https://status.nixos.org/prometheus/api/v1/query?query=channel_revision' \
    | jq ".data.result | map(.metric) | map(select(.channel == \"nixos-${CHANNEL}\"))[0].revision" -r)"
  if [[ $BASE = "null" ]]; then
    echo 'Base extraction failed' > /dev/stderr
    exit 1
  fi
  echo "$BASE"
}

commits() {
    (BRANCH="$1"
     cd "$NIXPKGS"
     set +o pipefail
     git log --pretty='format:author %ae%nhash %H' "$BRANCH" \
         | awk 'BEGIN { n=0; }; /^author/ { if ($2 != "sveina@gmail.com" && $2 != "svein@google.com" && n > 0) { exit } } /^hash/ { print $2; n++; }'
    )
}

update() {
    if [[ $(find "$HERE/.base" -mtime +0 2>&1 | wc -l) -gt 0 ]]; then
        BASE="$(get_base)"
        echo $BASE > "$HERE/.base"
    else
        BASE="$(cat $HERE/.base)"
    fi
    WANTED="$(mktemp)"
    echo "Building nix-system tree from base $BASE..."
    cat "$HERE/CHERRY_PICKS" | while read branch; do
        if ! [[ "$branch" =~ '^#' ]]; then
            LIST="$(mktemp)"
            commits "$branch" > "$LIST"
            echo "  plus $(wc -l $LIST | awk '{print $1}') commit(s) from $branch"
            tac "$LIST" >> "$WANTED"
            rm "$LIST"
        fi
    done
    HASH="$((echo "$BASE"; cat "$WANTED") | sha256sum | awk '{print $1}')"
    if [[ "$HASH" != "$(cat "$HERE/.state")" ]]; then
        echo 'Hash changed; rebuilding git tree.'
        (cd "$NIXPKGS"
         git reset --hard --merge -q; git clean -fxd
         git cat-file -t "$BASE" 2>/dev/null >/dev/null || git fetch origin
         git checkout "$BASE" -q
         git branch -D system -q
         git checkout -b system -q
         for cp in $(cat "$WANTED"); do
           git cherry-pick $cp >/dev/null
         done
        )
        echo "$HASH" > "$HERE/.state"
    fi
    rm "$WANTED"
}

{
  if [[ "${1:-}" = "--update" ]]; then
    shift
    BASE="$(get_base)"
    if [[ "$BASE" != "$(cat $HERE/.base)" ]]; then
      echo "$BASE" > "$HERE/.base"
    else
      echo "Base didn't change"
      exit 0
    fi
  fi

  update


  exit

  nixops deploy -d personal --check -j 8 --cores 16 -I "nixpkgs=$NIXPKGS" --build-only "$@"

  echo 'Spreading secrets.'
  for machine in secrets/shared/*; do
    machine=$(basename $machine)
    rsync --timeout=10 --delete-after -av secrets/shared/"$machine"/ "root@$machine".brage.info:/secrets/ || true &
  done
  wait

  echo 'Deploying.'
  nixops modify -d personal ./personal.nix
  nixops deploy -d personal --check -j 8 --cores 16 -I "nixpkgs=$NIXPKGS" "$@"

  echo 'Updating home-manager.'
  nixops ssh-for-each "sudo -su svein bash -c 'home-manager switch'" -d personal "$@"

  echo
  echo "Booted kernel:  $(readlink /run/booted-system/kernel | awk -F/ '{print $4}')"
  echo "Current kernel: $(readlink /run/current-system/kernel | awk -F/ '{print $4}')"
}
