#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JSON_FILE="$SCRIPT_DIR/modules/defaultApps.json"

if [ $# -eq 0 ]; then
    echo "Usage: $0 <package-name>"
    echo "Add a package to the defaultApps.json list"
    exit 1
fi

PACKAGE="$1"

if [ ! -f "$JSON_FILE" ]; then
    echo "Error: $JSON_FILE not found!"
    exit 1
fi

# Check if package already exists
if jq -e --arg pkg "$PACKAGE" 'any(. == $pkg)' "$JSON_FILE" > /dev/null 2>&1; then
    echo "Package '$PACKAGE' already exists in the list"
    exit 0
fi

# Add the package and sort the list
jq --arg pkg "$PACKAGE" '. += [$pkg] | sort' "$JSON_FILE" > "$JSON_FILE.tmp" && mv "$JSON_FILE.tmp" "$JSON_FILE"

echo "Added '$PACKAGE' to defaultApps.json"