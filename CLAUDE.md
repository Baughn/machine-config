# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Project: NixOS Configuration

Updates and modifications are handled from saya. tsugumi.local and v4.brage.info
can be contacted through ssh.

This repository contains NixOS configuration files for three machines:
- **saya**: Desktop system (AMD 7950X3D + RTX 4090, gaming/workstation)
- **tsugumi**: Server system (ZFS storage, various services)
- **v4**: IPv4 proxy server

## Important Context

### Migration Status
**CRITICAL**: This repository is actively migrating from an older configuration. Before making any migration-related changes, read `MIGRATION_PLANS.md` for:
- List of pending migrations with priority levels
- Technical notes about specific services
- Missing module imports and their purposes

### Architecture Overview
The configuration uses:
- **Nix Flakes** for reproducible builds
- **Colmena** for deployment management
- **Modular design** with shared modules in `modules/`
- **Machine-specific** configurations in `machines/` (saya, tsugumi, v4)
- **Agenix** for secrets management (encrypted .age files in `secrets/`)
- **Custom tools** in `tools/` (Rust-based services and utilities)

## Essential workflows

### Adding a package, tool, piece of software or service

1. Use mcp-nixos to check for a NixOS option for the piece of software. Servers are typically in the service hierarchy. Programs (steam, mtr, etc.) are typically under programs.
2. Use the service/programs option, if one exists. Offer suggestions as to potential extra configuration that might be useful.
3. If and ONLY if there is no such option, then use the mcp-nixos package search. Assuming a package is found, use ./add-package.sh to add it; then run `colmena apply`.

## Essential Commands

### Build and Deploy
```bash
# Check configuration validity
nix flake check

# Build and view changes
colmena apply --on saya    # Deploy to specific machine
colmena apply              # Deploy to all machines
colmena apply-local --sudo # Deploy to current machine only

# View what would change
nixos-rebuild dry-activate --flake .#hostname
```

### Linting and Formatting

Runs automatically; fix lints if they arise.

### Testing
```bash
# Run comprehensive checks
nix flake check

# Run VM tests
nix build .#tests.basic-desktop.x86_64-linux
```

### Development Tools
```bash
# Search NixOS options (ALWAYS use before adding/editing options)
./tools-for-claude/search-options.sh search <term>
./tools-for-claude/search-options.sh info <option.path>

# Add a new package
./add-package.sh <package-name>

# Access nixpkgs source for reference
cd ~/dev/nixpkgs/
```

## Version Control
**IMPORTANT**: This project uses Jujutsu (jj) instead of Git. DO NOT use git commands.

```bash
jj status          # Show working copy changes
jj diff            # Show diff of changes
jj commit -m "feat(module): Add feature"  # Commit with Conventional Commits format
jj squash          # Squash into previous commit
jj log --limit 5   # Show recent commits
jj undo            # Undo last operation if mistake made
```

### Commit Message Format
Use Conventional Commits specification:
- `feat(scope):` New feature
- `fix(scope):` Bug fix
- `chore(scope):` Maintenance
- `refactor(scope):` Code restructuring
- `docs(scope):` Documentation

## Code Style and Conventions

### Nix Style
- Use `./tools-for-claude/lint.sh` before committing (runs statix, deadnix, and formatter)
- Module options should use the `me` namespace for custom options
- Prefer `lib.mkOption` with clear descriptions and types
- Use `lib.mkIf` for conditional configurations

### Anchor Comments
Use specially formatted comments for inline knowledge:
- `AIDEV-NOTE:` Important implementation details
- `AIDEV-TODO:` Pending tasks
- `AIDEV-QUESTION:` Clarification needed

**Important**: Before modifying code, search for existing `AIDEV-*` anchors in relevant files. Update anchors when changing associated code.

### Module Organization
- Shared modules in `modules/` export options under `me.*`
- Machine configs import modules and set machine-specific values
- Application lists in `modules/cliApps.json` and `modules/desktopApps.json`
- Hardware quirks in `quirks/` for specific hardware issues

## Secrets Management
- Secrets are managed with agenix
- Encrypted `.age` files in `secrets/`
- Only secrets for the current host are decrypted
- Never commit unencrypted secrets
- Host public keys in `machines/*/ssh_host_ed25519_key.pub`

## Machine-Specific Notes

### saya (Desktop)
- Gaming optimizations with GameMode and AMD X3D quirks
- Restic backups to tsugumi every 30 minutes
- Logitech G903 mouse scroll fix applied
- Core pinning for V-Cache optimization

### tsugumi (Server)
- ZFS filesystem (migration pending)
- Hosts various services (see MIGRATION_PLANS.md)
- NVIDIA persistence daemon for GPU
- Target for backup storage

### v4 (Proxy)
- Simple IPv4 proxy using custom Rust tool
- Minimal configuration

## Common Tasks

### Adding a New Module
1. Create module file in `modules/`
2. Add to imports in `modules/default.nix`
3. Use `me.*` namespace for options
4. Run `./tools-for-claude/lint.sh`

### Adding a New Machine
1. Generate SSH keys: `ssh-keygen -t ed25519`
2. Create `machines/hostname/configuration.nix`
3. Add to `flake.nix` under `nixosConfigurations`
4. Add to `colmena` in `flake.nix`
5. Configure in `secrets/secrets.nix` if using secrets

### Updating Dependencies
```bash
python update.py  # Interactive update process; must be run by user on their own
# OR manually:
nix flake update
nix build .#nixosConfigurations.hostname.config.system.build.toplevel
```

## Important Files
- `flake.nix` - Main entry point and system definitions
- `update.py` - Automated update script with diff viewing
- `modules/default.nix` - Core module importing all others
- `MIGRATION_PLANS.md` - Critical migration tracking document
- `secrets/secrets.nix` - Age encryption key management

## Troubleshooting
- New files break the build until committed with `jj commit`
- Use `nix flake check` to validate configuration
- Check `jj status` before committing to ensure all files are tracked
- For option errors, use `search-options.sh` to verify correct syntax
- Deployment failures: check machine connectivity and SSH access

## Additional Context
- Jumbo frames enabled for local network (9000 MTU)
- Distributed builds planned but not yet configured
- Many services pending migration from old configuration
- Custom Rust tools in `tools/` have their own Cargo.toml files
