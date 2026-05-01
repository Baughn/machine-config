# NixOS/nix-darwin multi-machine configuration

## Architecture

Three-layer design supporting NixOS and nix-darwin from a single repo:

### 1. Machine configurations (`machines/<name>/default.nix`)
Machine-specific settings: hardware, networking, hostname, and values for custom module options.
Each machine imports `modules/default.nix` plus its own `hardware-configuration.nix`.

Machines:
- **saya** — Desktop (NixOS, x86_64-linux). CachyOS kernel, NVIDIA GPU, KDE Plasma 6.
- **tsugumi** — Server (NixOS, x86_64-linux). IPv6-only.
- **v4** — IPv4 proxy (NixOS, x86_64-linux). Forwards IPv4 traffic to tsugumi.
- **kaho** — Laptop (nix-darwin, aarch64-darwin). macOS with home-manager.

### 2. Module option declarations (`modules/default.nix`)
Shared entry point imported by all machines regardless of platform.
This file ONLY imports module subdirectories — it defines no config itself.

### 3. Platform-specific module implementations (`modules/<name>/`)
Each module subdirectory contains:
- `nixos.nix` — NixOS implementation (always present for NixOS-only modules)
- `darwin.nix` — nix-darwin implementation (present when the module supports macOS; assert-false stub when it doesn't make sense on macOS)

A library helper `mkPlatformModule` selects the correct file at eval time based on
`pkgs.stdenv.isDarwin`. The wrong platform's file is never evaluated — this is critical
because even referencing a nonexistent option (behind mkIf) is a compile error.

Module options should be system-agnostic where feasible. Platform files provide the
`config` implementation for those options.

### What to modularize

There is exactly one machine of each type (one desktop, one server, one proxy, one laptop).
Settings specific to a machine *type* are effectively machine-specific and belong in the
machine config — no module needed. Only extract into a module when the config is genuinely
shared (or shareable) across machines. Examples:

- **Module-worthy:** shell/zsh setup, CLI tools, nix settings, DNS, SSH auth
- **Machine-specific:** desktop environment, GPU drivers, boot loader, game clients, server services

## Build & Deploy

- **saya (local):** `./rebuild.sh` then `sudo systemctl restart display-manager` if DE changes
- **Remote machines:** colmena (planned)
- **kaho:** `darwin-rebuild switch --flake .#kaho`

## Flake structure

```
flake.nix
machines/
  saya/default.nix
  tsugumi/default.nix
  v4/default.nix
  kaho/default.nix
modules/
  default.nix          # imports all module subdirs via mkPlatformModule
  dns/
    nixos.nix
    darwin.nix
  desktop/
    nixos.nix
  ...
lib/
  default.nix          # mkPlatformModule and other helpers
```

## Conventions

- Module options live under the `me.*` namespace (e.g., `me.dns.upstream`).
- Options use `mkEnableOption` / `mkOption` with sensible defaults.
- Machine configs should be thin: set option values, import hardware config, done.
- No `with pkgs;` at module level — use `pkgs.foo` explicitly for clarity.
- Keep nixpkgs on unstable channel.

## Practical advice

- If responding to a request from Discord, always end a session with ./rebuild.sh to
  activate the changes.
- Assume this repository was written by an absent-minded programmer in a hurry. The docs
  do not necessarily match reality, and if you spot a mismatch you should always ask
  which to fix.
