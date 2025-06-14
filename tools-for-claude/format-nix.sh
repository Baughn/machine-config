#!/usr/bin/env bash
# Format all Nix files in the repository

set -euo pipefail

echo "Formatting all .nix files..."
find . -name "*.nix" -exec nixpkgs-fmt {} +
echo "Done formatting Nix files."
