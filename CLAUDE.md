# NixOS configuration

## Status

Today this repo configures three NixOS machines:

- **saya** — Desktop (NixOS, x86_64-linux). CachyOS kernel, NVIDIA GPU, KDE Plasma 6. *(here today)*
- **tsugumi** — Server (NixOS, x86_64-linux). ZFS storage, WireGuard hub, web/media/game/bot services.
- **v4** — IPv4 proxy (NixOS, x86_64-linux). Forwards IPv4 traffic to tsugumi.
- **kaho** — Laptop (nix-darwin, aarch64-darwin). macOS with home-manager. *(planned)*

Multi-platform support was scaffolded once and removed: the abstraction
(`mkPlatformModule`, paired `nixos.nix` / `darwin.nix` files) wasn't paying
for itself with no darwin machine actually present. We'll reintroduce a
platform split when kaho lands and a real darwin module forces concrete
requirements.

## Architecture

### 1. Machine configurations (`machines/<name>/default.nix`)
Machine-specific settings: hardware, networking, hostname, and values for
custom module options. Each machine imports `modules` (all shared modules)
plus its own `hardware-configuration.nix` and any machine-only feature
files (e.g. `machines/saya/steam.nix`).

### 2. Shared modules (`modules/<name>.nix`)
Each module is a single flat NixOS module that declares options under
`me.*` and provides `config` behind those options. `modules/default.nix`
is just an `imports` list that pulls them all in.

### What to modularize

There is exactly one machine of each type (one desktop, one server, one
proxy, one laptop). Settings specific to a machine *type* are effectively
machine-specific and belong in the machine config — no module needed. Only
extract into a module when the config is genuinely shared (or shareable)
across machines.

- **Module-worthy:** shell/zsh setup, CLI tools, nix settings, DNS, SSH auth, home-manager
- **Machine-specific:** desktop environment, GPU drivers, boot loader, game clients, server services

## Build & Deploy

- **saya (local):** `./rebuild.sh`, then `sudo systemctl restart display-manager` if DE changes.
- **Remote machines:** plain `nixosConfigurations` outputs today; colmena is not currently wired in.
- **kaho (planned):** likely a separate `darwinConfigurations.kaho` output. Adding it will
  require deciding how Linux-only modules opt out — `lib.mkIf pkgs.stdenv.isLinux` inside
  each module works; a `pkgs`-conditional `imports` list does *not* (it recurses through
  config). A separate `modules/darwin.nix` entry point that imports a subset is also viable.

## Flake structure

```
flake.nix
machines/
  saya/default.nix
  saya/hardware-configuration.nix
  saya/<feature>.nix       # cachy-tweaks, ganbot, game-watcher, steam, ...
  tsugumi/default.nix
  tsugumi/hardware-configuration.nix
  tsugumi/<service>.nix     # caddy, minecraft, monitoring, redis, rendezvous, ...
  v4/default.nix
  v4/hardware-configuration.nix
  v4/v4proxy.nix
modules/
  default.nix              # plain imports list
  agenix.nix
  cli-tools.nix
  dns.nix
  firejail.nix
  home-manager.nix
  mdns.nix
  nix.nix
  security.nix
  shell.nix
  ssh.nix
  wireguard.nix
lib/
  ssh-keys.nix              # shared authorized-key lists for explicit users
secrets/
  secrets.nix
  *.age
```

## Conventions

- Module options live under the `me.*` namespace (e.g. `me.wireguard.peers`).
- Options use `mkEnableOption` / `mkOption` with sensible defaults.
- Machine configs should be thin: set option values, import hardware config, done.
- No `with pkgs;` at module level — use `pkgs.foo` explicitly for clarity. Exception:
  `with pkgs;` is fine inside a package list (e.g. `environment.systemPackages = with pkgs; [ ripgrep htop ];`)
  where the scope is obvious and limited.
- Modules that apply identical config to every machine can be unconditional (no
  `me.X.enable` toggle); add a toggle when a machine actually wants the module off.
  This is now tested against saya, tsugumi, and v4, but still revisit before
  adding darwin support for kaho.
- Keep nixpkgs on unstable channel.

## Practical advice

- The repository uses Jujutsu. Git is available, but not preferred.
- Assume this repository was written by an absent-minded programmer in a hurry. The docs
  do not necessarily match reality, and if you spot a mismatch you should always ask
  which to fix.
