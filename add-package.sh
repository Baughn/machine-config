#!/usr/bin/env nix-shell
#!nix-shell -i bash -p jq

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_JSON_FILE="$SCRIPT_DIR/modules/cliApps.json"
DESKTOP_JSON_FILE="$SCRIPT_DIR/modules/desktopApps.json"

if [ $# -eq 0 ]; then
    echo "Usage: $0 <package-name>"
    echo "Add a package to the appropriate JSON file (CLI or desktop)"
    exit 1
fi

PACKAGE="$1"

# Check if package exists in nixpkgs
echo "Checking if '$PACKAGE' exists in nixpkgs..."
if ! nix-instantiate --eval -E "with import <nixpkgs> {}; pkgs.lib.hasAttrByPath [\"$PACKAGE\"] pkgs" | grep -q "true"; then
    echo "Error: Package '$PACKAGE' not found in nixpkgs"
    echo "Make sure the package name is correct and exists in the nixpkgs repository"
    exit 1
fi

# Function to check if a package is already in a JSON file
check_if_exists() {
    local file="$1"
    local pkg="$2"
    jq -e --arg pkg "$pkg" 'any(. == $pkg)' "$file" > /dev/null 2>&1
}

# Check if package already exists in either list
if check_if_exists "$CLI_JSON_FILE" "$PACKAGE" || check_if_exists "$DESKTOP_JSON_FILE" "$PACKAGE"; then
    echo "Package '$PACKAGE' already exists in one of the lists"
    exit 0
fi

# Function to add a package to a JSON file
add_package() {
    local file="$1"
    local pkg="$2"
    jq --arg pkg "$pkg" '. += [$pkg] | unique | sort' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    echo "Added '$PACKAGE' to $file"
}

# Check if the package has a dependency on X11 or Wayland
echo "Analyzing dependencies for '$PACKAGE'..."
nix build "nixpkgs#$PACKAGE"
REFERENCES=$(nix path-info --recursive "nixpkgs#$PACKAGE")

if echo "$REFERENCES" | grep -q -e "libX11" -e "wayland"; then
    echo "'$PACKAGE' appears to be a desktop application."
    add_package "$DESKTOP_JSON_FILE" "$PACKAGE"
else
    echo "'$PACKAGE' appears to be a CLI application."
    add_package "$CLI_JSON_FILE" "$PACKAGE"
fi
