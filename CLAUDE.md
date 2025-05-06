# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Overview

This repository contains NixOS configurations for multiple machines, organized as a Nix flake. It includes configurations for several systems (tsugumi, saya, kaho, v4), shared modules for common functionality, and custom installer configurations.

## Architecture

- **Flake-based**: The repository uses Nix flakes for reproducible builds and dependency management
- **Multi-host**: Configurations for different machines (tsugumi, saya, v4, etc.)
- **Modular**: Common functionality is extracted into reusable modules
- **Secrets management**: Uses agenix for encrypted secrets
- **Home-manager integration**: Includes home-manager configurations for user environments

## Common Commands

### Building and Updating

```bash
# Build the current configuration without applying it
nixos-rebuild --flake . build

# Test the configuration
nixos-rebuild --flake . test

# Update all flake inputs and try to build
./update.sh

# Build and switch to the new configuration
sudo nixos-rebuild --flake . switch

# Build but apply only after reboot
sudo nixos-rebuild --flake . boot
```

### Working with Specific Hosts

```bash
# Build configuration for a specific host
nixos-rebuild --flake .#<hostname> build

# Switch to configuration for a specific host
sudo nixos-rebuild --flake .#<hostname> switch
```

### Installer Operations

```bash
# Build the installer CD image
nix build '.#install-cd'

# Build the kexec installer
nix build '.#install-kexec'

# Update the installer on a Ventoy device (requires Ventoy device to be connected)
./update-installer.sh
```

### Development

```bash
# Enter development shell
nix develop

# Format code (using Alejandra formatter)
nix fmt

# Decrypt secrets (requires proper keys)
agenix -d <secret-file>.age
```

## Secrets Management

The repository uses agenix for secrets management. Secrets are stored encrypted in the `secrets/` directory. To work with secrets:

1. The decryption requires proper SSH keys defined in `age.identityPaths` in the flake
2. Secrets can be decrypted with `agenix -d <secret-file>.age`
3. New secrets can be added using `agenix -e <secret-file>.age`

## Module Structure

- `modules/`: Contains reusable NixOS modules
  - `basics.nix`: Common system configuration applied to all hosts
  - `desktop.nix`: GUI-related configuration
  - `zfs.nix`, `wireguard.nix`, etc.: Specific service configurations
  - `ipblock/`: Network blocking configuration

- Each host has its own directory with:
  - `configuration.nix`: Main system configuration
  - `hardware-configuration.nix`: Hardware-specific settings
  - Additional service configurations

## Home Manager Configuration

The user configuration is defined in `home/home.nix` and is integrated into each machine configuration through the `homeConfig` list in the flake.