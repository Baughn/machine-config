#!/usr/bin/env bash

set -euo pipefail

# Function to run jq - uses system jq if available, otherwise nix-shell
jq() {
    if command -v jq >/dev/null 2>&1; then
        command jq "$@"
    else
        nix-shell -p jq --run "jq $(printf '%q ' "$@")" 2>/dev/null
    fi
}

# Colors
BLUE='\033[1;34m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
RESET='\033[0m'

# Help text
usage() {
    cat <<EOF
Usage: $0 [search|info] <term>

Search for NixOS options by name and display their details.

Commands:
  search <term>  List option names matching the search term
  info <term>    Show detailed information for matching options

The search is case-insensitive and matches partial option names.

Options:
  -h, --help     Show this help message

Examples:
  $0 search boot.loader
  $0 info networking.firewall.enable
  $0 search services.openssh

Additional features for Claude:
  - Output is limited to prevent overwhelming context
  - Use 'search' mode to find option names first
  - Use 'info' mode for detailed information on specific options
  - Exact matches are shown first in info mode
EOF
}

# Parse arguments
if [[ $# -eq 0 ]] || [[ "$1" == "-h" ]] || [[ "$1" == "--help" ]]; then
    usage
    exit 0
fi

MODE="$1"
if [[ "$MODE" != "search" ]] && [[ "$MODE" != "info" ]]; then
    echo -e "${RED}Error: Unknown command '$MODE'${RESET}" >&2
    echo "Use 'search' or 'info'" >&2
    exit 1
fi

if [[ $# -lt 2 ]]; then
    echo -e "${RED}Error: Missing search term${RESET}" >&2
    usage
    exit 1
fi

SEARCH_TERM="$2"

# Build options documentation
echo -e "${YELLOW}Building options documentation...${RESET}" >&2
OPTIONS="$(nix build --no-link --print-out-paths .#options 2>/dev/null)"

# Create temporary file for output
TMPFILE=$(mktemp)
trap "rm -f $TMPFILE" EXIT

# Maximum output size (in bytes) - ~50KB should be reasonable for Claude
MAX_SIZE=50000

if [[ "$MODE" == "search" ]]; then
    # Search mode: just list matching option names
    echo -e "${GREEN}Options matching '$SEARCH_TERM':${RESET}" >&2
    
    jq -r --arg search "$SEARCH_TERM" '
      keys |
      map(select(. | ascii_downcase | contains($search | ascii_downcase))) |
      if length == 0 then
        "No options found matching \"" + $search + "\""
      else
        .[]
      end
    ' "$OPTIONS" > "$TMPFILE"
    
else
    # Info mode: show detailed information
    echo -e "${GREEN}Detailed information for options matching '$SEARCH_TERM':${RESET}" >&2
    
    jq -r --arg search "$SEARCH_TERM" '
      # First try exact match
      if has($search) then
        [{"key": $search, "value": .[$search]}]
      else
        # Otherwise do partial match
        to_entries |
        map(select(.key | ascii_downcase | contains($search | ascii_downcase))) |
        sort_by(.key)
      end |
      if length == 0 then
        "No options found matching \"" + $search + "\""
      else
        .[] | 
        "\n\u001b[1;34m" + .key + "\u001b[0m" +
        if .value.description then
          "\n  Description: " + (.value.description | gsub("\n"; " "))
        else "" end +
        if .value.type then
          "\n  Type: " + .value.type
        else "" end +
        if .value.default then
          "\n  Default: " + (
            if .value.default._type == "literalExpression" then
              .value.default.text
            else
              .value.default | tojson
            end
          )
        else "" end +
        if .value.example then
          "\n  Example: " + (
            if .value.example._type == "literalExpression" then
              .value.example.text
            else
              .value.example | tojson
            end
          )
        else "" end +
        if .value.readOnly then
          "\n  Read-only: true"
        else "" end +
        if .value.declarations then
          "\n  Declared in: " + (.value.declarations | 
            if type == "array" and (.[0] | type) == "string" then
              join(", ")
            elif type == "array" and (.[0] | type) == "object" then
              map(.name) | join(", ")
            else
              tojson
            end
          )
        else "" end +
        "\n"
      end
    ' "$OPTIONS" > "$TMPFILE"
fi

# Check output size
FILE_SIZE=$(stat -c%s "$TMPFILE" 2>/dev/null || stat -f%z "$TMPFILE" 2>/dev/null || echo 0)

if [[ $FILE_SIZE -gt $MAX_SIZE ]]; then
    echo -e "${RED}Error: Output too large ($(($FILE_SIZE / 1024))KB)${RESET}" >&2
    echo -e "${YELLOW}Please use a more specific search term.${RESET}" >&2
    
    # Show count of matches
    if [[ "$MODE" == "search" ]]; then
        COUNT=$(wc -l < "$TMPFILE")
        echo -e "${YELLOW}Found $COUNT matching options.${RESET}" >&2
    else
        # Count options in info mode
        COUNT=$(grep -c "^$(printf '\033')\[1;34m" "$TMPFILE" || true)
        echo -e "${YELLOW}Found $COUNT matching options with full details.${RESET}" >&2
    fi
    
    # Show first few matches as a sample
    echo -e "\n${GREEN}First 10 matches; use search mode to find more:${RESET}" >&2
    if [[ "$MODE" == "search" ]]; then
        head -n 10 "$TMPFILE"
    else
        # For info mode, show just the option names
        grep "^$(printf '\033')\[1;34m" "$TMPFILE" | head -n 10 | sed 's/\x1b\[[0-9;]*m//g'
    fi
    
    exit 1
fi

# Output the results
cat "$TMPFILE"
