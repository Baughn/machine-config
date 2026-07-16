# nix-deploy: unified build + deploy tool

*Last updated: 2026-07-16. Status: implemented (v1); this doc matches the
code.*

## Problem

`deploy-all.sh` today does `nix build .#all-systems | nom --json` followed by
`colmena apply`. This has two costs:

1. **Double evaluation.** The flake is evaluated once for the `nix build` and
   again inside colmena — ~20 seconds each.
2. **nom slows the build.** nom's rendering is expensive enough (Haskell,
   eager redraws) that piping the JSON log through it measurably slows the
   build it is displaying.

Separately, neither tool knows anything about *what changed*: a kernel,
NVIDIA-driver, or KWin bump silently gets `switch`ed, leaving the machine in a
state where a reboot is needed but nothing says so.

## Goals

- One flake evaluation total, shared by every machine.
- A nom-style build progress tree whose rendering can never block the build.
- Closure-diff inspection per machine, with a reboot-worthiness verdict and an
  interactive **switch / boot / boot+reboot / exit** choice.
- Sequenced deployment: remote machines (tsugumi) first, the local machine
  (saya) last, since a local `switch` (or reboot) can disrupt the session
  driving the deploy.
- Replaces both nom and colmena in the daily workflow.

## Non-goals

- Clever transport or orchestration. `nix copy` over ssh on a LAN plus
  `switch-to-configuration` is the whole push mechanism.
- Rollback automation. `nixos-rebuild --rollback` / boot-menu rollback and the
  existing `magic-reboot` module remain the recovery story.
- Secrets, provisioning, multi-user deploys, non-NixOS targets.
- Removing colmena from `flake.nix` immediately — it stays as a fallback until
  the tool has earned trust (see Migration).

## Shape

A Rust crate at `tools/nix-deploy/` (the existing directory is an abandoned
scaffold containing only a `target/` dir — delete and reuse the name), built
with `pkgs.mkCranePackage` like the other tools, installed on saya via
`environment.systemPackages`. Binary name: `nix-deploy`.

```
nix-deploy [MACHINE...] [--mode switch|boot]
```

- No machine arguments: deploy every machine in the manifest, remotes first,
  local host last.
- `MACHINE...`: deploy only the named machines (still in canonical order).
- `--mode`: skip the interactive prompt and force that mode on every machine.
  Never auto-reboots anything; with `--mode switch` the reboot verdict is
  still printed as a warning.

Must be run from the repo root (or with the repo as an argument later, if
ever needed — not in v1).

## Machine manifest

A small `deploy.toml` at the repo root, read by the tool:

```toml
[machines.tsugumi]
target = "svein@tsugumi.local" # ssh destination; also used for nix copy
order = 1

[machines.saya]
local = true                   # activate via sudo, no ssh; deploys last
order = 2
```

Rationale: the alternative — reading the machine list out of the flake —
costs an extra evaluation or an eval-time JSON output, exactly what we're
trying to eliminate. Two machines in a TOML file is not a maintenance burden.
The manifest is the single place that knows how tsugumi is reached;
everything else derives from it. Root commands are wrapped in `sudo`
(svein has passwordless sudo on both machines, and root ssh is disabled);
an explicit `sudo = false` or a `root@` target skips the wrapping.

## Pipeline

### 1. Build (single eval)

```
nix build --no-link --log-format internal-json \
  .#nixosConfigurations.saya.config.system.build.toplevel \
  .#nixosConfigurations.tsugumi.config.system.build.toplevel
```

One `nix` process evaluates once (in-process eval cache is shared across
installables) and builds both toplevels. Out-paths come from
`--print-out-paths` on stdout, matched to machines by their
`nixos-system-<name>-` store names. stderr carries the JSON log that feeds
the progress UI.

`.#all-systems` in `flake.nix` becomes redundant for deploys; keep or drop it
independently.

### 2. Progress UI (nom-style, but skippable)

Two threads:

- **Parser thread** reads stderr line-by-line, decodes `@nix` JSON messages
  (`start`/`stop`/`result` activities), and maintains the model: derivations
  building (with elapsed time and last log line), downloads in flight
  (bytes/total), counts of done/expected, failures with captured log tails.
  This thread must keep up with nix at full speed; it does no I/O to the
  terminal.
- **Render thread** wakes at most ~12 Hz, snapshots the model (shared via a
  mutex; the parser holds it only per-line), and redraws a status tree:
  running builds with elapsed time, phase, and last log line, downloads with
  byte progress, a recent-completions line, and a summary header. If a frame
  is late, it is skipped — the model is always current, frames are
  disposable. Rendering cost therefore bounds *display latency*, never build
  throughput.

Non-TTY output (piped, CI): internal-json is not requested at all; nix's own
non-tty log format streams through untouched.

On build failure: print the failed derivation's log tail (nix includes it in
the JSON stream), exit non-zero. Nothing has been pushed yet.

### 3. Closure diff + reboot verdict (per machine)

For each machine, compare the freshly built toplevel against what is running:

- **Old system path:** locally, `readlink -f /run/current-system`; remotely,
  the same over ssh.
- **Old closure listing:** `nix-store --query --requisites` on the old path —
  run remotely over ssh for tsugumi (the old closure need not exist in saya's
  store). Only store path *names* are needed, not contents. (`nix-store`
  rather than `nix path-info`: no experimental-features dependency.)
- **New closure listing:** the same, locally on the new toplevel.

Two classes of checks:

**Built-in structural checks** — compare well-known symlinks inside the two
toplevels (`readlink`, remote via ssh for the old side):

| Check | Signal |
|---|---|
| `kernel` | kernel update |
| `kernel-modules` | module set changed — this also catches NVIDIA driver bumps |
| `initrd` | initrd rebuilt |
| `systemd` (from closure pattern `systemd-<ver>`) | pid-1 update; `switch` re-execs systemd but a reboot is cleaner |

**Pattern checks** — regexes matched against store-path *names* in both
closures; a pattern whose matched set differs between old and new is
reboot-worthy. Built-in defaults: `nvidia-x11`, `linux-firmware`. Per-machine
extras come from the machine config (see next section) — e.g. saya adds
`kwin` while the local-KWin-build investigation is live.

The verdict plus its reasons ("kernel 6.15.4 → 6.15.6", "kwin changed") is
printed alongside a short human diff. For the human diff, shell out to
`nvd diff <old> <new>` if available rather than reimplementing it; it's
purely informational. (For tsugumi, `nvd` needs both closures locally — if
the old closure isn't in saya's store, skip nvd and show only the tool's own
verdict reasons. Don't copy closures just to prettify output.)

### 4. Reboot patterns from machine config, without a second eval

A tiny module, `modules/nix-deploy.nix`, declares:

```nix
me.deploy.rebootPatterns = mkOption {
  type = types.listOf types.str;
  default = [];
}
```

and embeds the result *into the system closure itself*:

```nix
system.systemBuilderCommands = ''
  cp ${pkgs.writeText "deploy-reboot-patterns.json"
        (builtins.toJSON config.me.deploy.rebootPatterns)} \
     $out/deploy-reboot-patterns.json
'';
```

The tool reads `<toplevel>/deploy-reboot-patterns.json` from the path it just
built. Policy lives in nix, travels with the build, and costs zero extra
evaluations. Machines that don't set it get `[]` plus the built-ins.

### 5. Prompt

After the diff, per machine, on saya's terminal:

```
tsugumi: reboot recommended
  kernel: 6.15.4 → 6.15.6
  kernel-modules changed
  [s]witch anyway  [b]oot only  [r] boot + reboot now  [e]xit
```

- **switch** — activate now despite the verdict.
- **boot** — `switch-to-configuration boot`; takes effect at next manual
  reboot.
- **boot + reboot** — for tsugumi: `boot`, then `systemctl reboot` over ssh,
  then wait for it to return (see below). For saya: `boot`, then
  `systemctl reboot` locally — the tool prints a farewell and the session
  ends; this is only offered as the *last* machine, which saya always is.
- **exit** — abort the whole run. Machines already activated stay activated;
  everything is idempotent on rerun (builds cached, `nix copy` no-ops,
  re-switching to the same system is harmless).

When the verdict is "no reboot needed", the prompt collapses to a simple
confirm-or-exit (or nothing at all — see Open questions).

### 6. Activation

Per machine, in manifest order (tsugumi, then saya):

1. `nix copy --to ssh://root@tsugumi <toplevel>` (skipped for local).
2. Prompt (above) unless `--mode` given.
3. Set the profile so rollback and GC behave:
   `nix-env --profile /nix/var/nix/profiles/system --set <toplevel>`
   (over ssh as root; locally via sudo).
4. `<toplevel>/bin/switch-to-configuration <mode>` (ssh/sudo likewise).
   Stream its output through; a non-zero exit is a deploy failure.
5. If boot+reboot on tsugumi: `systemctl reboot`, then poll ssh every few
   seconds until reachable, then require
   `systemctl is-system-running --wait` to report `running` (or `degraded`,
   with a loud warning listing failed units), and verify
   `readlink /run/current-system` equals the new toplevel. Timeout: 5
   minutes, configurable in `deploy.toml`.

**Failure policy: abort everything.** Any failure — copy, activation, reboot
timeout — stops the run before later machines are touched. Rerunning after a
fix is cheap since every earlier step is idempotent. (`magic-reboot` remains
the big hammer if tsugumi wedges.)

ssh invocations use the manifest's `target` string and inherit the user's ssh
config/agent — no credentials handling in the tool. Root operations (profile
set, activation, reboot) go through `sudo` on both machines; svein's sudo is
passwordless, so nothing interrupts the flow.

## Crate layout

```
tools/nix-deploy/
  Cargo.toml          # anyhow, clap, serde/serde_json, regex, toml, terminal_size
  src/
    main.rs           # CLI, orchestration
    manifest.rs       # deploy.toml
    build.rs          # nix build invocation, out-path recovery
    log_model.rs      # internal-json parser + build-state model
    ui.rs             # render thread, tree drawing, non-TTY fallback
    diff.rs           # closure listing, structural + pattern checks, verdict
    target.rs         # ssh/local command abstraction (run, copy, reboot-wait)
    activate.rs       # profile set + switch-to-configuration + reboot flow
```

`log_model.rs` and `diff.rs` are pure and unit-tested against captured
`internal-json` samples and synthetic closure lists; `check-rust-tools`
covers the crate like every other tool. `agents/rust.md` applies.

## Migration

1. Land the crate + `modules/nix-deploy.nix` + `deploy.toml`; install the
   binary on saya.
2. Run it alongside the scripts for a while. `deploy-all.sh` shrinks to
   `exec nix-deploy "$@"`; `deploy-tsugumi.sh` → `exec nix-deploy tsugumi`;
   `deploy-local.sh` → `exec nix-deploy saya`.
3. Once trusted: remove colmena and `colmenaHive` from `flake.nix`, drop the
   nom dependency from the scripts (or the scripts entirely), delete the
   untracked `nix-output-monitor/` checkout.

## Open questions

- **Prompt on no-verdict machines:** when nothing is reboot-worthy, should the
  tool still pause for confirmation before switching tsugumi, or just go?
  Leaning "just go" — the whole point is an unattended happy path — but it
  changes the feel of the tool. Default in v1: just go, print what it did.
- **`is-system-running` = `degraded`:** treat as success-with-warning (v1
  choice above) or failure? tsugumi has enough services that a single flaky
  unit shouldn't strand a deploy.
- **Eval progress display:** `internal-json` eval activity reporting is
  coarser than build reporting; the 20-second eval may show as little more
  than a spinner. Acceptable for v1.
