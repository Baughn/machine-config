# NixOS Repository Architecture Documentation

This document provides a comprehensive overview of the NixOS repository structure, module system, and workflows for managing a multi-machine NixOS configuration using Nix Flakes and Colmena deployment.

## Table of Contents

1. [Repository Overview](#repository-overview)
2. [Architecture Principles](#architecture-principles)
3. [Directory Structure](#directory-structure)
4. [Module System Design](#module-system-design)
5. [Machine Configuration Model](#machine-configuration-model)
6. [Application Management System](#application-management-system)
7. [Secrets Management](#secrets-management)
8. [Build and Deployment Workflow](#build-and-deployment-workflow)
9. [Adding New Components](#adding-new-components)
10. [Common Patterns and Examples](#common-patterns-and-examples)
11. [Troubleshooting](#troubleshooting)

## Repository Overview

This repository manages NixOS configurations for three machines using a modular, flake-based approach:

- **saya**: Desktop/workstation (AMD 7950X3D + RTX 4090, gaming/development)
- **tsugumi**: Server system (ZFS storage, various services)
- **v4**: IPv4 proxy server (minimal configuration)

### Key Technologies

- **Nix Flakes**: Reproducible, hermetic builds with locked dependencies
- **Colmena**: Multi-machine deployment management
- **Agenix**: Age-based secrets encryption
- **Home Manager**: User environment management
- **Custom Modules**: Reusable configuration components with the `me.*` namespace

## Architecture Principles

### 1. Modular Design
- Shared functionality isolated in reusable modules
- Machine-specific configurations import only what they need
- Clear separation between core functionality and machine-specific customizations

### 2. The `me.*` Namespace
All custom options use the `me.*` namespace to avoid conflicts with upstream NixOS options:

```nix
# In a module
options.me.myService.enable = lib.mkOption {
  type = types.bool;
  default = false;
  description = "Enable my custom service";
};

# In machine configuration
me.myService.enable = true;
```

### 3. Declarative Package Management
Applications are managed through JSON lists that are automatically converted to package installations:
- `modules/cliApps.json`: Command-line tools available on all machines
- `modules/desktopApps.json`: GUI applications for desktop machines

### 4. Layered Configuration
Configuration flows from general to specific:
1. **Base modules** (`modules/default.nix`): Core functionality for all machines
2. **Specialized modules** (`modules/desktop.nix`): Role-specific functionality  
3. **Machine configurations** (`machines/*/configuration.nix`): Machine-specific settings
4. **Hardware quirks** (`quirks/`): Hardware-specific fixes

## Directory Structure

```
/home/svein/nixos/
├── flake.nix                    # Main entry point, defines all configurations
├── flake.lock                   # Locked dependency versions
├── modules/                     # Shared configuration modules
│   ├── default.nix              # Core modules imported by all machines
│   ├── cliApps.json            # CLI applications for all machines
│   ├── desktopApps.json        # Desktop applications
│   ├── desktop.nix             # Desktop environment configuration
│   ├── users.nix               # User account management
│   ├── networking.nix          # Network configuration options
│   └── ...                     # Other specialized modules
├── machines/                    # Machine-specific configurations
│   ├── saya/                   # Desktop machine
│   │   ├── configuration.nix   # Main machine config
│   │   └── hardware-configuration.nix
│   ├── tsugumi/                # Server machine
│   └── v4/                     # Proxy server
├── secrets/                     # Age-encrypted secrets
│   ├── secrets.nix             # Defines which secrets go to which machines
│   └── *.age                   # Encrypted secret files
├── tools/                       # Custom Rust applications
│   ├── updater/                # System update tool
│   ├── ping-discord/           # Discord notification tool
│   └── ...                     # Other custom tools
├── quirks/                      # Hardware-specific fixes
│   ├── amd-x3d.nix             # AMD X3D CPU optimizations
│   └── g903.nix                # Logitech mouse fixes
├── tests/                       # NixOS VM tests
├── home/                        # Home Manager configuration
└── context/                     # Documentation
```

## Module System Design

### Core Module Pattern

All modules follow a consistent structure:

```nix
{ config, lib, pkgs, ... }:
with lib;
{
  # Define options under the me.* namespace
  options.me.myModule = {
    enable = mkEnableOption "my custom module";
    
    setting = mkOption {
      type = types.str;
      default = "default-value";
      description = "A configurable setting";
    };
  };

  # Implementation when enabled
  config = mkIf config.me.myModule.enable {
    # NixOS configuration here
    services.someService.enable = true;
    environment.systemPackages = with pkgs; [ some-package ];
  };
}
```

### Module Import Chain

1. **flake.nix** defines the base configuration for all machines
2. **modules/default.nix** imports core modules used by all machines
3. **Machine configurations** import additional specialized modules
4. **Modules** can import other modules to build on shared functionality

### Key Modules

#### `modules/default.nix` - Core System
- Imports fundamental modules (users, networking, monitoring, etc.)
- Sets up Nix configuration (flakes, garbage collection, etc.)
- Installs CLI applications from `cliApps.json`
- Configures SSH, DNS resolution, and shell defaults

#### `modules/desktop.nix` - Desktop Environment
- Imports desktop-specific performance tuning
- Configures display manager (SDDM) and Plasma 6
- Sets up audio (PipeWire), gaming optimizations
- Installs desktop applications from `desktopApps.json`

#### `modules/users.nix` - User Management
- Defines all system users with UIDs and SSH keys
- Provides `users.include` option to selectively enable users per machine
- Centralizes SSH key management through `modules/keys.nix`

## Machine Configuration Model

Each machine follows this pattern:

```nix
{ config, lib, pkgs, ... }:
{
  imports = [
    ./hardware-configuration.nix    # Hardware-specific settings
    ../../modules                   # Core modules
    ../../modules/desktop.nix       # Role-specific modules (if applicable)
    ../../quirks/some-quirk.nix    # Hardware quirks (if needed)
  ];

  # Machine-specific configuration
  networking.hostName = "machine-name";
  
  # Enable specific users
  users.include = [ "svein" ];
  
  # Machine-specific services, networking, etc.
}
```

### Machine Profiles

#### Saya (Desktop)
```nix
imports = [
  ./hardware-configuration.nix
  ../../modules                 # Core functionality
  ../../modules/desktop.nix     # Desktop environment
  ../../modules/nvidia.nix      # NVIDIA GPU support
  ../../modules/secure-boot.nix # Secure boot with Lanzaboote
  ../../quirks/g903.nix         # Mouse scroll fix
  ../../quirks/amd-x3d.nix      # CPU topology optimization
];
```

#### Tsugumi (Server)  
```nix
imports = [
  ./hardware-configuration.nix
  ../../modules                 # Core functionality only
  # Various service-specific configurations
  ./caddy.nix
  ./minecraft.nix
  # ... other services
];
```

#### V4 (Proxy)
```nix
imports = [
  ./hardware-configuration.nix
  ../../modules                 # Minimal core functionality
  ../../modules/v4proxy.nix     # Custom proxy service
];
```

## Application Management System

### CLI Applications (`modules/cliApps.json`)

```json
[
  "bat",
  "fd", 
  "git",
  "htop",
  "jq",
  "ripgrep"
]
```

These packages are automatically installed on all machines via:

```nix
# In modules/default.nix
environment.systemPackages = with pkgs;
  let
    cliApps = builtins.fromJSON (builtins.readFile ./cliApps.json);
  in
  map (name: pkgs.${name}) cliApps;
```

### Desktop Applications (`modules/desktopApps.json`)

Similar to CLI apps but only installed on machines that import `modules/desktop.nix`.

### Adding Packages

Use the provided script for automatic categorization:

```bash
./add-package.sh package-name
```

The script:
1. Verifies the package exists in nixpkgs
2. Analyzes dependencies to determine if it's CLI or desktop
3. Adds to the appropriate JSON file
4. Prevents duplicates

## Secrets Management

### Agenix Integration

Secrets are encrypted using Age with SSH keys and managed through `secrets/secrets.nix`:

```nix
{
  "restic.pw.age".publicKeys = all;              # Available to all machines
  "caddy.env.age".publicKeys = host tsugumi;     # Only tsugumi + admin users
  "rolebot-config.json.age".publicKeys = host tsugumi;
}
```

### Secret Usage in Configuration

```nix
# Reference secrets in configuration
services.restic.backups.home = {
  passwordFile = config.age.secrets."restic.pw".path;
  # ... other config
};
```

### Key Management

- User SSH keys defined in `modules/keys.nix`
- Machine host keys in `machines/*/ssh_host_ed25519_key.pub`
- Secrets only decrypted on machines that need them

## Build and Deployment Workflow

### Local Development

```bash
# Check configuration validity
nix flake check

# Build specific machine configuration  
nix build .#nixosConfigurations.saya.config.system.build.toplevel

# Test in VM
nix build .#tests.basic-desktop.x86_64-linux
```

### Deployment with Colmena

```bash
# Deploy to specific machine
colmena apply --on saya

# Deploy to all machines  
colmena apply

# Deploy to current machine only (for local changes)
colmena apply-local --sudo

# Preview changes without applying
colmena apply --dry-run
```

### Colmena Configuration

The flake defines deployment targets in `colmenaHive`:

```nix
colmenaHive = colmena.lib.makeHive {
  meta = {
    nixpkgs = import nixpkgs { /* config */ };
    specialArgs = { inherit inputs; };
  };

  defaults = { /* Common configuration */ };

  saya = {
    imports = [ ./machines/saya/configuration.nix ];
    deployment = {
      targetHost = "localhost";
      allowLocalDeployment = true;
    };
  };

  tsugumi = {
    imports = [ ./machines/tsugumi/configuration.nix ];
    deployment = {
      targetHost = "tsugumi.local";
      tags = [ "remote" ];
    };
  };
};
```

## Adding New Components

### Adding a New Module

1. **Create the module file** in `modules/`:

```nix
# modules/my-service.nix
{ config, lib, pkgs, ... }:
with lib;
{
  options.me.myService = {
    enable = mkEnableOption "my custom service";
    port = mkOption {
      type = types.port;
      default = 8080;
      description = "Port to run the service on";
    };
  };

  config = mkIf config.me.myService.enable {
    systemd.services.my-service = {
      description = "My Custom Service";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        ExecStart = "${pkgs.my-package}/bin/my-service --port ${toString config.me.myService.port}";
        Restart = "always";
      };
    };
  };
}
```

2. **Add to imports** in `modules/default.nix`:

```nix
imports = [
  # ... existing imports
  ./my-service.nix
];
```

3. **Enable in machine configuration**:

```nix
me.myService = {
  enable = true;
  port = 9090;
};
```

### Adding a New Machine

1. **Generate SSH host key**:
```bash
ssh-keygen -t ed25519 -f machines/newmachine/ssh_host_ed25519_key
```

2. **Create machine configuration**:
```nix
# machines/newmachine/configuration.nix
{ config, lib, pkgs, ... }:
{
  imports = [
    ./hardware-configuration.nix
    ../../modules
    # ... other modules as needed
  ];

  networking.hostName = "newmachine";
  users.include = [ "svein" ];
  
  # Machine-specific configuration
  system.stateVersion = "25.05";
}
```

3. **Add to flake.nix**:
```nix
newmachine = {
  imports = [ ./machines/newmachine/configuration.nix ];
  deployment = {
    targetHost = "newmachine.example.com";
    tags = [ "remote" ];
  };
};
```

4. **Update secrets** if needed in `secrets/secrets.nix`.

### Adding Custom Tools

Custom tools are Rust applications in the `tools/` directory:

1. **Create tool directory**:
```bash
mkdir tools/my-tool
cd tools/my-tool
cargo init
```

2. **Create `default.nix`**:
```nix
{ pkgs, ... }:

pkgs.rustPlatform.buildRustPackage {
  pname = "my-tool";
  version = "0.1.0";

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
```

3. **Use in machine configuration**:
```nix
environment.systemPackages = with pkgs; [
  (callPackage ../../tools/my-tool { })
];
```

## Common Patterns and Examples

### Conditional Module Imports

```nix
# Import module only if condition is met
imports = [
  ../../modules
] ++ lib.optional (config.networking.hostName == "saya") ../../modules/desktop.nix;
```

### Hardware-Specific Quirks

```nix
# quirks/my-hardware.nix
{ config, lib, pkgs, ... }:
{
  # Apply fixes for specific hardware
  boot.kernelParams = [ "some.parameter=value" ];
  environment.sessionVariables = {
    SOME_VARIABLE = "hardware-specific-value";
  };
}
```

### Service Configuration Patterns

```nix
# Pattern for configurable services  
{ config, lib, pkgs, ... }:
with lib;
{
  options.me.myService = {
    enable = mkEnableOption "my service";
    
    settings = mkOption {
      type = types.submodule {
        options = {
          host = mkOption {
            type = types.str;
            default = "localhost";
          };
          port = mkOption {
            type = types.port;
            default = 8080;
          };
        };
      };
      default = {};
    };
  };

  config = mkIf config.me.myService.enable {
    systemd.services.my-service = {
      # Service definition using config.me.myService.settings.*
    };
  };
}
```

### Package Overlays

```nix
# In a module that needs custom packages
nixpkgs.overlays = [
  (final: prev: {
    my-custom-package = prev.callPackage ./path/to/package.nix { };
  })
];
```

## Troubleshooting

### Common Issues

**Build Failures**
- Run `nix flake check` to validate configuration
- Check that all imported files exist and are tracked by git/jj
- Verify module imports are correct

**Deployment Failures**  
- Check network connectivity to target machines
- Verify SSH access and host keys
- Ensure target machine has sufficient disk space

**Module Errors**
- Use the MCP NixOS tools to verify option names:
  ```bash
  ./tools-for-claude/search-options.sh search <term>
  ./tools-for-claude/search-options.sh info <option.path>
  ```

**Secret Access Issues**
- Verify the machine's host key is included in `secrets/secrets.nix`
- Check that the secret file exists and is properly encrypted
- Ensure the secret is referenced correctly in configuration

### Debugging Tools

```bash
# View what would change
nixos-rebuild dry-activate --flake .#hostname

# Build without deploying
nix build .#nixosConfigurations.hostname.config.system.build.toplevel

# Check flake inputs and outputs
nix flake show
nix flake metadata

# Validate JSON application lists
jq . modules/cliApps.json
jq . modules/desktopApps.json
```

### Development Workflow

1. **Make changes** to modules or machine configurations
2. **Lint code**: `./tools-for-claude/lint.sh`
3. **Test locally**: `nix flake check`
4. **Deploy**: `colmena apply --on machine-name`
5. **Commit changes**: `jj commit -m "feat(module): description"`

This documentation provides a complete mental model of how the NixOS configuration system is organized, enabling efficient navigation, modification, and extension of the system.