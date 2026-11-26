#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

jj git fetch
jj rebase -r 'remote_bookmarks()..@' -d 'trunk()'
