# NixOS configuration

## Status

Today this repo configures two NixOS machines, plus a rescue image:

- **saya** — Desktop (NixOS, x86_64-linux). CachyOS kernel, NVIDIA GPU, KDE Plasma 6. *(here today)*
- **tsugumi** — Server (NixOS, x86_64-linux). ZFS storage, WireGuard hub, web/media/game/bot services.
- **saya-installer** — Netboot rescue/installer image for saya. Built as a separate
  `nixosConfigurations` output (not a Colmena node) and covered by a VM test in
  `tests/saya-installer-vm.nix` (`nix flake check`).
- **kaho** — Laptop (nix-darwin, aarch64-darwin). macOS with home-manager. *(planned)*

Former machines: **v4**, an IPv4-proxy VPS forwarding to tsugumi, was dropped
(flake in May 2026, remaining files in June 2026). Its config and the v4proxy
Rust crate live on in git history if a successor appears; `modules/ssh-auth.nix`
(the OTP module only v4 enabled) is kept in-tree but currently unused.

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

### 3. Rust tools (`tools/<name>/`)
Each tool is a Cargo crate built with crane via `pkgs.mkCranePackage`
(overlay defined in `flake.nix`, implementation in `lib/mk-crane-package.nix`).
Tools are wired into machines by `machines/<machine>/<tool>.nix` or a
`modules/<tool>.nix` module. `nix develop` provides a Rust toolchain and a
`check-rust-tools` script that runs `cargo test` across every crate.

### What to modularize

There is exactly one machine of each type (one desktop, one server, one
laptop). Settings specific to a machine *type* are effectively
machine-specific and belong in the machine config — no module needed. Only
extract into a module when the config is genuinely shared (or shareable)
across machines.

- **Module-worthy:** shell/zsh setup, CLI tools, nix settings, DNS, SSH, home-manager, magic-reboot
- **Machine-specific:** desktop environment, GPU drivers, boot loader, game clients, server services

## Build & Deploy

- **saya (local):** `./deploy-local.sh`, then `sudo systemctl restart display-manager` if DE changes.
- **tsugumi (remote):** Colmena is wired in through `colmenaHive`; use `./deploy-tsugumi.sh`
  or `./deploy-all.sh`.
- **VCS sync:** `./push.sh` (squash into the running "Bumps" commit, set master, git push;
  pings Discord on minecraft/ssh-key changes), `./pull.sh` (fetch + rebase onto trunk).
- **Tests:** `nix flake check` builds the machines and runs the saya-installer VM test.
- **kaho (planned):** likely a separate `darwinConfigurations.kaho` output. Adding it will
  require deciding how Linux-only modules opt out — `lib.mkIf pkgs.stdenv.isLinux` inside
  each module works; a `pkgs`-conditional `imports` list does *not* (it recurses through
  config). A separate `modules/darwin.nix` entry point that imports a subset is also viable.

## KWin debugging (temporary)

saya currently builds KWin from the local `kwin/` checkout (source override in
the saya overlay in `flake.nix`) as part of an active investigation into an
NVIDIA atomic-modeset failure — see `kwin-bug/README.md` and `kwin-bug/TODO.md`.
saya also imports `kwin-bug/drm-atomic-log.nix`, an LD_PRELOAD shim that logs
DRM atomic ioctls from the compositor. Don't touch `kwin/`,
`kwin-6.6.3-original/`, or `kwin.patch` unless working on that bug; when the
fix lands upstream, the overlay and the drm-atomic-log import should both go.

## Flake structure

```
flake.nix                  # inputs, machine list, packages, checks, devShell, colmenaHive
machines/
  saya/default.nix
  saya/hardware-configuration.nix
  saya/<feature>.nix       # ganbot, game-watcher, steam, restic
  saya-installer/default.nix
  tsugumi/default.nix
  tsugumi/hardware-configuration.nix
  tsugumi/<service>.nix    # caddy, minecraft, monitoring, redis, rendezvous, sonarr, ...
modules/
  default.nix              # plain imports list
  agenix.nix
  cachy-kernel.nix         # CachyOS kernel + tuning, heavily stripped config
  cli-tools.nix
  cloudflare-dyndns.nix
  dns.nix
  firejail.nix
  home-manager.nix
  kernel-modules.nix       # hard-block blacklisted modules via modprobe install rules
  magic-reboot.nix         # keyed magic-packet emergency reboot, on by default
  mdns.nix
  nix.nix
  nix-build-balancer.nix
  remote-builds.nix
  security.nix
  shell.nix
  ssh-auth.nix             # OTP-gated SSH password auth (currently unused)
  ssh.nix
  svein.nix
  wireguard.nix
  zfs.nix
lib/
  ssh-keys.nix             # shared authorized-key lists for explicit users
  mk-crane-package.nix     # crane wrapper behind pkgs.mkCranePackage
tools/                     # Rust crates: aniwatch, game-watcher, irc-tool,
                           # magic-reboot, nix-build-balancer, rolebot, victron-monitor
tests/
  saya-installer-vm.nix
secrets/
  secrets.nix
  *.age
kwin-bug/                  # NVIDIA modeset bug investigation (see above)
agents/rust.md             # Rust code-quality guidelines for AI agents
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
  This is now tested against saya and tsugumi, but still revisit before
  adding darwin support for kaho.
- Keep nixpkgs on unstable channel.

## Practical advice

- This repository uses Jujutsu. Prefer `jj` for history/status/log operations when practical; use Git only when a workflow specifically needs it.
  Note: even read-only `jj` commands may write snapshots into `.git/objects`, so Codex needs sandbox escalation for them. These are automatically granted.
  Note: `jj help [command]` for flags, `jj help -k [possible values: bookmarks, config, filesets, glossary, revsets, templates, tutorial]` for other info.
- Assume this repository was written by an absent-minded programmer in a hurry. The docs
  do not necessarily match reality, and if you spot a mismatch you should always ask
  which to fix.
