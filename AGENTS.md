# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# Project: Hybrid System Configuration

Updates and modifications are handled from saya. tsugumi.local and v4.brage.info
can be contacted through ssh.

This repository manages three machines with a hybrid approach:
- **saya**: Desktop (AMD 7950X3D + RTX 4090) — **CachyOS** + Ansible + home-manager standalone
- **tsugumi**: Server (ZFS storage, various services) — **NixOS** via colmena
- **v4**: IPv4 proxy server — **NixOS** via colmena

## Important Context

### Current machine

All work happens on saya. If you see this, then chances are you're running on saya.

### Architecture Overview

**saya (CachyOS):**
- System packages: `pacman`/`paru` (no declarative package list)
- System config: **Ansible** playbooks in `ansible/` (WireGuard, firewall, users, services)
- User config: **home-manager standalone** (`home-manager switch --flake .#svein`)
- Kernel/desktop/drivers: handled natively by CachyOS (sched-ext, gamemode, mhwd)

**tsugumi + v4 (NixOS):**
- **Nix Flakes** for reproducible builds
- **Colmena** for deployment management
- **Modular design** with shared modules in `modules/`
- Machine-specific configurations in `machines/` (tsugumi, v4)
- **Agenix** for secrets management (encrypted .age files in `secrets/`)

**Shared across all:**
- **home-manager** for user-level config (fish, zsh, git, jujutsu, SSH, neovim, tmux, direnv)
- Custom tools in `tools/` (Rust-based services and utilities)

## Essential workflows

### Adding a package to NixOS machines (tsugumi, v4)

1. Use mcp-nixos to check for a NixOS option for the piece of software. Servers are typically in the service hierarchy. Programs (steam, mtr, etc.) are typically under programs.
   ALWAYS do this, even if you think you know the correct config options. Keeping the defaults is often fine.
2. Use the service/programs option, if one exists. Offer suggestions as to potential extra configuration that might be useful.
3. If and ONLY if there is no such option, then use the mcp-nixos package search. Assuming a package is found, use ./add-package.sh to add it; then run `colmena apply`.

Note: A programs.<program>.enable entry will do the equivalent of add-package; don't use both.

### Adding a package to saya (CachyOS)

Use `paru -S <package>` or `pacman -S <package>`. No declarative config needed.

### Adding user-level config (applies to saya)

Edit files in `home/`. For saya-only config, guard with `lib.optionals isStandalone` or `lib.optionalAttrs isStandalone`.

### Adding system-level config to saya

Edit Ansible roles in `ansible/roles/` or add new roles to `ansible/site.yml`.

## Essential Commands

### Build and Deploy (NixOS machines)
```bash
# Check configuration validity
nix flake check

# Deploy to remote NixOS machines
colmena apply --on @remote

# Build all NixOS systems
nom build .#all-systems
```

### Update saya (CachyOS)
```bash
# System update
sudo cachy-update

# Home-manager
home-manager switch --flake .#svein

# Ansible (system config)
ansible-playbook ansible/site.yml
ansible-playbook --check ansible/site.yml  # dry run
```

### Full Update (all machines)
```bash
python update.py  # Interactive: flake update, build, deploy menu
```

### Linting and Formatting

Runs automatically; fix lints if they arise.

### Testing
```bash
# Run comprehensive checks
nix flake check  # This runs automatically on stop; there is normally no need to do this manually.
```

### Development Tools
```bash
# Add a new package (NixOS machines only)
./add-package.sh <package-name>

# Access nixpkgs source for reference
cd ~/dev/nixpkgs/
```

## Code Style and Conventions

### Nix Style
- Use `./tools-for-claude/lint.sh` before committing (runs statix, deadnix, and formatter)
- Module options should use the `me` namespace for custom options
- Prefer `lib.mkOption` with clear descriptions and types
- Use `lib.mkIf` for conditional configurations

### Module Organization
- Shared NixOS modules in `modules/` export options under `me.*`
- Machine configs import modules and set machine-specific values
- Application lists in `modules/cliApps.json` and `modules/desktopApps.json` (NixOS only)
- Hardware quirks archived in `archive/saya-nixos/` (reference only)

### Ansible Style
- Roles in `ansible/roles/`, one per concern
- Templates use `.j2` extension
- Variables in `ansible/group_vars/all.yml` and `ansible/host_vars/saya.yml`
- System secrets via Ansible vault; user secrets via agenix + home-manager

### Home-manager Organization
- Shared config in `home/home.nix` (used by NixOS, Darwin, and standalone)
- Standalone-only modules: `home/neovim.nix`, `home/tmux.nix`, `home/zsh-ohmyzsh.nix`
- Guard saya-specific config with `isStandalone`

## Secrets Management
- **NixOS machines**: agenix (encrypted `.age` files in `secrets/`)
- **saya user secrets**: agenix via home-manager standalone (restic.pw, magic-reboot sender key)
- **saya system secrets**: Ansible vault (WireGuard private key, magic-reboot listener key)
- Never commit unencrypted secrets

## Machine-Specific Notes

### saya (Desktop — CachyOS)
- CachyOS kernel with sched-ext (replaces NixOS zen kernel + scx_bpfland)
- GameMode with V-Cache core pinning (via Ansible `gamemode` role)
- NVIDIA drivers via mhwd (replaces modules/nvidia.nix)
- Restic backups to tsugumi every 30 minutes (via Ansible `restic-backup` role)
- Logitech G903 mouse scroll fix (via Ansible `hardware-quirks` role)
- WireGuard VPN to tsugumi (via Ansible `wireguard` role)
- G-Sync/VRR and WINE_CPU_TOPOLOGY env vars (via home-manager sessionVariables)
- Custom Rust tools (ping-discord, network-monitor) via home-manager packages

### tsugumi (Server — NixOS)
- ZFS filesystem (migration pending)
- Hosts various services (see MIGRATION_PLANS.md)
- NVIDIA persistence daemon for GPU
- Target for backup storage

### v4 (Proxy — NixOS)
- Simple IPv4 proxy using custom Rust tool
- Minimal configuration

## Common Tasks

### Adding a New NixOS Module
1. Create module file in `modules/`
2. Add to imports in `modules/default.nix`
3. Use `me.*` namespace for options
4. Run `./tools-for-claude/lint.sh`

### Adding a New Ansible Role
1. Create role directory in `ansible/roles/<name>/`
2. Add `tasks/main.yml` (and templates/, handlers/ as needed)
3. Add role to `ansible/site.yml`
4. Add variables to `ansible/host_vars/saya.yml`

### Adding a New Machine
1. Generate SSH keys: `ssh-keygen -t ed25519`
2. Create `machines/hostname/configuration.nix`
3. Add to `flake.nix` under `machineConfigs`
4. Configure in `secrets/secrets.nix` if using secrets

### Updating Dependencies
```bash
python update.py  # Interactive update process; must be run by user on their own
# OR manually:
nix flake update
nom build .#all-systems
```

## Important Files
- `flake.nix` - Main entry point and system definitions
- `update.py` - Automated update script (NixOS + CachyOS dual-mode)
- `modules/default.nix` - Core NixOS module importing all others
- `home/home.nix` - Shared home-manager configuration
- `home/neovim.nix` - Standalone neovim config (saya/Darwin)
- `home/tmux.nix` - Standalone tmux config (saya/Darwin)
- `ansible/site.yml` - Ansible master playbook for saya
- `MIGRATION_PLANS.md` - Critical migration tracking document
- `secrets/secrets.nix` - Age encryption key management
- `archive/saya-nixos/` - Archived NixOS config for saya (reference only)

## Troubleshooting
- New files break the build until committed with `jj commit`
- Use `nix flake check` to validate configuration
- Check `jj status` before committing to ensure all files are tracked
- For option errors, use `search-options.sh` to verify correct syntax
- Deployment failures: check machine connectivity and SSH access
- Ansible dry-run: `ansible-playbook --check ansible/site.yml`

## Additional Context
- Jumbo frames enabled for local network (9000 MTU)
- Custom Rust tools in `tools/` have their own Cargo.toml files
- `machines/saya/` directory still exists but is archived — do not modify

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
