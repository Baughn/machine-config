# Project: NixOS Configuration

This repository contains NixOS configuration files.

## Coding style

## Anchor comments  

Add specially formatted comments throughout the codebase, where appropriate, for yourself as inline knowledge that can be easily `grep`ped for.  

### Guidelines:  

- Use `AIDEV-NOTE:`, `AIDEV-TODO:`, or `AIDEV-QUESTION:` (all-caps prefix) for comments aimed at AI and developers.  
- Keep them concise (≤ 120 chars).  
- **Important:** Before scanning files, always first try to **locate existing anchors** `AIDEV-*` in relevant subdirectories.  
- **Update relevant anchors** when modifying associated code.  
- **Do not remove `AIDEV-NOTE`s** without explicit human instruction.  

## Version Control
This project uses Jujutsu (jj) for version management instead of Git.
A git repository is colocated (to make nix commands work), but DO NOT use git commands.

## Common Commands
- `jj status` - Show per-file working copy changes
- `jj diff` - Show contents of the working copy
- `jj commit -m "message"` -- Set commit message and create a new commit on top -- like `git commit`
- `jj log --limit 5` -- Show recent commits

## Testing
- `nix flake check` -- Comprehensive sanity check.

New files will break the build until after a commit.
