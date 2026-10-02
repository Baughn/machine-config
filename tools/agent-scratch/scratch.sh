# Throwaway directories an agent may create and delete without approval.
# A bare NAME lives in $AGENT_SCRATCH; rm also takes a path under /tmp or
# /var/tmp that the caller owns (mktemp -d leftovers). Nothing else.
usage() {
  echo "usage: scratch new NAME   (re)create an empty dir, print its path" >&2
  echo "       scratch rm NAME|/tmp/PATH...   delete scratch or own temp dirs" >&2
  echo "       scratch ls | clean" >&2
  exit 2
}

root=${AGENT_SCRATCH:?AGENT_SCRATCH is not set}
mkdir -p "$root"

# Resolve one argument to a path we may delete, or fail.
target() {
  local arg=$1 path
  if [[ $arg != */* ]]; then
    [[ $arg =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || { echo "scratch: bad name: $arg" >&2; return 1; }
    printf '%s\n' "$root/$arg"
    return
  fi
  path=$(realpath -e -- "$arg") || return 1
  if [[ $path != /tmp/?* && $path != /var/tmp/?* ]] || [[ $(stat -c %u -- "$path") != "$(id -u)" ]]; then
    echo "scratch: $arg is not a temp path of yours; ask for rm instead" >&2
    return 1
  fi
  printf '%s\n' "$path"
}

[[ $# -ge 1 ]] || usage
cmd=$1; shift
case $cmd in
  new)
    [[ $# -eq 1 && $1 != */* ]] || usage
    dir=$(target "$1")
    rm -rf --one-file-system -- "$dir"
    mkdir -- "$dir"
    printf '%s\n' "$dir"
    ;;
  rm)
    [[ $# -ge 1 ]] || usage
    for arg in "$@"; do
      dir=$(target "$arg")
      rm -rf --one-file-system -- "$dir"
    done
    ;;
  ls)
    [[ $# -eq 0 ]] || usage
    ls -la -- "$root"
    ;;
  clean)
    [[ $# -eq 0 ]] || usage
    find "$root" -mindepth 1 -maxdepth 1 -exec rm -rf --one-file-system -- {} +
    ;;
  *) usage ;;
esac
