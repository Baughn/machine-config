# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Project: NixOS updater

This is a multi-call binary used by various update workflows. It's not yet complete, but should include:
- A command that retrieves the currently built nixos-unstable (by default) commit.
- A command that shells out to Jujutsu to rebase WIP commits in ~/dev/nixpkgs/ on top of that commit.
- TBD
