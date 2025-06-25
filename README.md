# NixOS Configurations

This repository manages NixOS configurations for multiple machines using Nix Flakes.

## Project Overview

This setup allows for declarative, reproducible, and version-controlled system configurations. Key tools used include Nix, Nix Flakes, Colmena (for deployment), and Jujutsu (for version control).

## Repository Structure

*   `flake.nix`: The main entry point for the Nix Flake. It defines the NixOS systems (also known as "nodes" in Colmena context, e.g., `saya`, `v4`), common packages, and checks.
*   `saya/`: Contains the NixOS configuration specific to the machine named "saya" (likely a desktop).
    *   `configuration.nix`: Main configuration file for "saya".
    *   `hardware-configuration.nix`: Hardware-specific settings for "saya".
    *   `sdbot.nix`: Additional specific configurations for "saya".
*   `v4/`: Contains the NixOS configuration specific to the machine named "v4" (likely a server).
    *   `configuration.nix`: Main configuration file for "v4".
    *   `hardware-configuration.nix`: Hardware-specific settings for "v4".
*   `modules/`: A directory for shared NixOS modules that can be imported by any machine configuration.
    *   `default.nix`: Imports all modules within this directory.
    *   Other `.nix` files: Individual modules providing specific functionalities (e.g., `desktop.nix`, `nvidia.nix`).
*   `quirks/`: Contains configurations for hardware or software-specific workarounds and tweaks (e.g., `amd-x3d.nix`, `g903.nix`).
*   `tests/`: Basic VM-based tests to ensure configurations can build and boot.
*   `tools-for-claude/`: Utility scripts helpful for development and maintenance.
    *   `format-nix.sh`: Formats Nix code.
    *   `lint.sh`: Lints and formats code in the repository.
    *   `search-options.sh`: Searches NixOS configuration options.
*   `CLAUDE.md`: Contains instructions and guidelines for AI-assisted development with this repository.
*   `update.py`: A Python script to automate updating flake inputs and applying the configuration to the local machine ("saya").

## Prerequisites

*   **Nix Package Manager**: Ensure Nix is installed and Flakes are enabled.
    *   Refer to the [official Nix installation guide](https://nixos.org/download.html).
    *   Enable Flakes by adding `experimental-features = nix-command flakes` to your Nix configuration file (e.g., `~/.config/nix/nix.conf` or `/etc/nix/nix.conf`).
*   **Colmena**: Used for deploying NixOS configurations to local or remote machines.
    *   It's included as a system package for the "saya" machine (see `saya/configuration.nix` and `flake.nix`). For use outside of a configured system, it can typically be installed using `nix profile install .#colmena` from the root of this repository.
*   **Jujutsu (jj)**: This project uses Jujutsu for version control instead of Git.
    *   See [Jujutsu documentation](https://martinvonz.github.io/jj/latest/) for installation and usage.
*   **nvd**: (Optional, used by `update.py`) A tool to compare NixOS generations. `update.py` uses it to show what changed.

## Usage

### Updating System (Saya - Local Machine)

The `update.py` script automates the process of updating flake inputs, building the "saya" configuration, showing differences (using `nvd`), and applying it:

```bash
./update.py
```

This script will:
1.  Attempt to update all flake inputs (`nix flake update`).
2.  Run `nix flake check` to ensure basic validity.
3.  Build the "saya" configuration using `colmena build --on saya`.
4.  If the above fails, it attempts a selective update (excluding potentially problematic inputs like `nixpkgs-kernel`).
5.  Show system differences using `nvd diff /run/current-system <new_build_path>`.
6.  Prompt interactively to deploy the new configuration immediately or on the next boot.

### Building a Configuration

To build a configuration for a specific machine without applying it:

```bash
colmena build --on <machine_name>
```
Example: `colmena build --on saya` or `colmena build --on v4`.

To build for the local machine if its hostname matches a defined node:
```bash
colmena build
```

### Applying a Configuration

To apply a built configuration to a specific machine:

```bash
colmena apply --on <machine_name>
```
Example: `colmena apply --on v4`.

For the local machine (if its hostname matches a defined node):
```bash
colmena apply
```

To apply the configuration on the next boot:
```bash
colmena apply boot --on <machine_name>
# Or for the local machine:
colmena apply boot
```

## Development

### Version Control

This project uses **Jujutsu (jj)** for version control.
**Important**: Do NOT use `git` commands directly for committing or branching, as `jj` manages the underlying Git repository. The Git repository is primarily for compatibility with tools that expect it (like `nix flake`).

Common `jj` commands:
*   `jj status`: Show per-file working copy changes.
*   `jj diff`: Show contents of the working copy changes.
*   `jj commit -m "message"`: Create a new commit.
*   `jj log`: Show commit history.

Refer to `CLAUDE.md` for more context or the official Jujutsu documentation.

### Linting and Formatting

Before committing changes, run the linting and formatting script:

```bash
./tools-for-claude/lint.sh
```
This script typically formats Nix files and might include other checks.

### Searching NixOS Options

To find and inspect documentation for NixOS configuration options, use the provided script:

*   Search for option names:
    ```bash
    ./tools-for-claude/search-options.sh search <term>
    ```
    Example: `./tools-for-claude/search-options.sh search networking.firewall`

*   Get detailed info about specific options:
    ```bash
    ./tools-for-claude/search-options.sh info <option_name>
    ```
    Example: `./tools-for-claude/search-options.sh info services.openssh.enable`

Always check option documentation before adding or modifying options, as they can change between Nixpkgs versions.

### Testing

Run comprehensive checks, including VM-based tests defined in `flake.nix`:

```bash
nix flake check
```
New files might cause build issues until they are committed (tracked by `jj`).

### Anchor Comments

This codebase uses specially formatted "anchor comments" (`AIDEV-NOTE:`, `AIDEV-TODO:`, `AIDEV-QUESTION:`) for AI and developer inline knowledge. See `CLAUDE.md` for guidelines on their use.

## Managing Modules

Shared configurations are organized into modules within the `modules/` directory.

*   `modules/default.nix` serves as an aggregate importer for all modules in this directory.
*   **To add a new shared module**:
    1.  Create your new `.nix` file (e.g., `modules/my-new-feature.nix`).
    2.  Define your NixOS options and configurations within this file.
    3.  Add your new module to the `imports` list in `modules/default.nix`. For example:
        ```nix
        # In modules/default.nix
        imports = [
          ./desktop.nix
          ./my-new-feature.nix # Add your new module here
          # ... other modules
        ];
        ```
    4.  The module's options and configurations will then be available to any machine that imports `../modules` or specific module files.

## Adding a New Machine

To add a new machine configuration to this Flake:

1.  **Create a Directory**: Create a new directory for the machine (e.g., `newmachine/`).
2.  **Add Configuration Files**:
    *   Inside `newmachine/`, create a `configuration.nix` file. This will hold the primary NixOS settings for the new machine.
    *   Create a `hardware-configuration.nix` file (often generated by `nixos-generate-config` during an initial NixOS install on the target hardware).
3.  **Define in `flake.nix`**:
    *   Open `flake.nix`.
    *   Locate the `colmenaHive` definition within the `outputs` section.
    *   Add a new attribute for your machine, similar to the existing `saya` or `v4` entries. This typically involves specifying the modules to import (including `./newmachine/configuration.nix` and any shared modules from `../modules`).
    *   Example structure:
        ```nix
        # In flake.nix, under outputs.colmenaHive
        # ...
        newmachine = { name, nodes, ... }: {
          imports = [
            ./newmachine/configuration.nix
            # Add other relevant modules, e.g.:
            # ../modules
            # nix-index-database.nixosModules.nix-index # If you want nix-index
          ];

          # Optional: machine-specific deployment settings
          # deployment = {
          #   targetHost = "newmachine.example.com";
          #   targetUser = "root";
          #   buildOnTarget = false;
          #   # ...
          # };

          # Optional: machine-specific nixpkgs or registry settings
          # nix.registry.nixpkgs.flake = nixpkgs;
        };
        # ...
        ```
4.  **Machine-Specific Quirks**: If the new machine has hardware or software that requires specific workarounds, consider adding a file to the `quirks/` directory and importing it in the machine's `configuration.nix` or directly in the `flake.nix` entry for that machine.
5.  **Test**: Build the configuration using `colmena build --on newmachine` and run `nix flake check`.
6.  **Deploy**: Once satisfied, deploy using `colmena apply --on newmachine`.
```
