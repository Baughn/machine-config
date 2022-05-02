#!/usr/bin/env bash

set -eu -o pipefail

cd "$(readlink -f "$(dirname "$0")")"
java -Xmx1200m -jar KanjiTomo.jar -run
