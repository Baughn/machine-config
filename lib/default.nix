{ lib }:

# Custom library functions for this NixOS configuration
#
# This module aggregates all custom library functions into a single namespace.

{
  # Version comparison and selection utilities
  versions = import ./versions.nix { inherit lib; };
}
