# Project: NixOS Configuration

This repository contains NixOS configuration files.

## Coding style

### Nix Code Formatting
Use the formatting tool to format all Nix files before committing:
```bash
./tools-for-claude/format-nix.sh
```

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

## Tools for Claude

### NixOS Options Search
Use `tools-for-claude/search-options.sh` to find and inspect the documentation for NixOS configuration options.
*Always* do this prior to adding or editing options. There may well be changes you are unaware of.

**Search for option names:**
```bash
./tools-for-claude/search-options.sh search <term>
```
Example: `./tools-for-claude/search-options.sh search networking.firewall`

**Get detailed info about options:**
```bash
./tools-for-claude/search-options.sh info <term>
```
Example: `./tools-for-claude/search-options.sh info services.openssh.enable`

The tool automatically limits output size to prevent overwhelming context. Use more specific search terms if you get a "too large" error.
