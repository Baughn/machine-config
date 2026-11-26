# deploy: unified build + deploy tool

*Deployment workflow updated: 2026-11-26 (interactive TUI, jj hygiene, speculative flake updates, input diff review, system telemetry). Status: implemented (v2). The crate lives in `tools/nix-deploy/`; the installed command is `deploy`.*

## Problem

The former `deploy-all.sh` workflow did `nix build .#all-systems | nom --json`
followed by `colmena apply`. That had two costs:

1. **Double evaluation.** The flake was evaluated once for `nix build` and
   again inside colmena — ~20 seconds each.
2. **nom slows the build.** nom's rendering was expensive enough (Haskell,
   eager redraws) that piping the JSON log through it measurably slowed the
   build it was displaying.

In v1, `deploy` eliminated both and added closure diffs + reboot verdicts.
However, daily deploys still required separate manual rituals around it:
`./pull.sh`, committing dirty jj working copies, running `nix flake update`,
inspecting third-party flake input diffs (overlays, custom modules, tools) for
supply-chain/breaking changes, and `./push.sh` before rebooting.

## Goals

- One flake evaluation total, shared by every machine.
- A full-screen `ratatui` TUI (with clean non-TTY / `--mode` / `--check` /
  `--no-tui` fallback so unattended deployers like `agent-ship` behave
  identically to v1).
- Built-in Jujutsu repo hygiene: rebase incoming `trunk()` commits, name dirty
  working copies so `nix build` always records a clean `self.rev`, manage the
  `"Bumps"` commit for `flake.lock` updates, roll back `"Bumps"` via
  `jj op restore` on abort, and offer pushing `master` before a local reboot or
  on exit.
- Speculative background `nix flake update` that never mutates `./flake.lock`
  until accepted, plus asynchronous diff fetching for non-core changed flake
  inputs while `nix build` runs in parallel.
- Conservative filtering of pure version/hash bump hunks (toggleable) and a
  post-filter changed-file cap (default 60, expandable) so human review stays
  focused on actual code/config changes.
- Activation gating whenever reviewable flake input diffs exist.
- Top-bar `btop`-style CPU and Memory sparklines (with ZFS ARC reclaimable
  memory accounting).
- Sequenced deployment: remote machines (tsugumi) first, the local machine
  (saya) last, since a local `switch` (or reboot) can disrupt the session
  driving the deploy.

## Non-goals

- Clever transport or orchestration. `nix copy` over ssh on a LAN plus
  `switch-to-configuration` is the whole push mechanism.
- System rollback automation. `nixos-rebuild --rollback` / boot-menu rollback
  and the existing `magic-reboot` module remain the recovery story (only the
  jj `"Bumps"` commit is offered for `jj op restore` on pre-activation abort).
- Secrets, provisioning, multi-user deploys, non-NixOS targets.

## Shape

A Rust crate at `tools/nix-deploy/`, built with `pkgs.mkCranePackage` like
the other tools, installed on saya via `environment.systemPackages`.
Binary name: `deploy`.

```
deploy [MACHINE...] [--mode switch|boot] [--check] [--strict] [--allow-rollback] [--no-tui]
```

- No machine arguments: deploy every machine in the manifest, remotes first,
  local host last.
- `MACHINE...`: deploy only the named machines (still in canonical order).
- `--mode`: skip the TUI and interactive prompts, forcing that mode on every
  machine. Never auto-reboots anything; with `--mode switch` the reboot verdict
  is still printed as a warning.
- `--check`: run only the lineage check (below) in plain mode, then stop
  without building.
- `--strict`: the lineage check also refuses unrecorded or dirty running
  revisions. For unattended deployers (`agent-ship`).
- `--allow-rollback`: deploy even when the lineage check refuses.
- `--no-tui`: force the plain v1 CLI pipeline even on an interactive terminal.

Whenever `!stdin.is_terminal() || !stderr.is_terminal() || --mode || --check || --no-tui`,
`deploy` bypasses the TUI, jj prompts, and flake update completely and executes
the plain v1 pipeline unchanged.

## Machine manifest

`deploy.toml` at the repo root:

```toml
[settings]
reboot_timeout_secs = 300
ignore_diff_repos = [
    "NixOS/nixpkgs",
    "DeterminateSystems/nixpkgs-weekly",
    "nix-community/nixpkgs.lib",
    "nix-community/home-manager",
    "nix-darwin/nix-darwin",
]
max_diff_files = 60

[machines.tsugumi]
target = "svein@tsugumi.local" # ssh destination; also used for nix copy
order = 1

[machines.saya]
local = true                   # activate via sudo, no ssh; deploys last
order = 2
```

- `ignore_diff_repos`: repositories (`owner/repo`, case-insensitive, including
  FlakeHub URLs like `DeterminateSystems/nixpkgs-weekly`) skipped when fetching
  input diffs. High-churn core inputs live here; everything else is fetched and
  reviewed.
- `max_diff_files`: post-filter substantive changed-file threshold above which
  an input's diff starts collapsed in the TUI (expandable with `e`).

## Interactive TUI workflow

### Top bar: CPU & Memory sparklines (`sysmon.rs`)

A full-width 3-line header samples Linux telemetry every 500 ms:

- **CPU**: delta busy percentage from `/proc/stat` + 1-minute load average from
  `/proc/loadavg` + 120-sample sparkline.
- **Memory**: `/proc/meminfo` (`MemTotal - MemAvailable`) minus ZFS ARC
  reclaimable bytes (`size.saturating_sub(c_min)` from
  `/proc/spl/kstat/zfs/arcstats`, matching `btop`/`htop`). Displays
  `MEM XX.XG (+YY.YG ARC) / ZZ.ZG`, omitting `(+ARC)` when ZFS is absent.

### Startup dialogs (`repo.rs`, `lineage.rs`, `flake_diff.rs`)

1. **Repo sync**: Inspects `.jj` via `repo::inspect`. If `@..trunk()` is
   non-empty, shows the incoming commits and offers `[r]ebase onto trunk`,
   `[c]ontinue without rebasing`, or `[q]uit`. Rebase conflicts exit cleanly
   with an error summary.
2. **Working copy hygiene**: Re-inspects after optional rebase. If `@` is dirty:
   - Only `flake.lock` modified: deferred to the `"Bumps"` step.
   - Unnamed dirty `@`: prompts for a commit message, then runs `jj describe` +
     `jj new`.
   - Named dirty `@`: automatically runs `jj new`.
   This guarantees `nix build` evaluates a clean commit (`self.rev`, not
   `-dirty`).
3. **Lineage check**: Verifies each selected machine's running
   `configurationRevision` is an ancestor (or jj rewrite) of the deploying
   revision. Refusals exit cleanly unless `--allow-rollback` is passed.
4. **Speculative `nix flake update`**: Immediately spawns `nix flake update`
   against a temporary lockfile in the background (`./flake.lock` is never
   touched until accepted; quitting or skipping kills the child and removes the
   temp directory).
   - While running: `[y]` queues auto-apply when ready, `[n]` skips, `[q]` quits.
   - If up to date: auto-dismisses and starts building.
   - If inputs changed: lists bumped inputs (tagged `[core]` vs `[review]`) and
     prompts `[y] Apply & build` / `[n] Skip & build` / `[q] Quit`.
   - On accept: records `pre_bump_op_id` (`jj op log`), atomically replaces
     `./flake.lock`, squashes into a mutable `"Bumps"` parent or creates a new
     `"Bumps"` commit + `jj new`, then launches background diff fetchers AND
     `nix build` in parallel.

### Main two-pane workspace

Responsive layout: 50/50 side-by-side when terminal width >= 100 columns,
single `Tab`-switched pane on narrower terminals.

- **Left pane (`Diffs & Commits`)**:
  - Section 1: Local commits since each machine's running revision
    (`git log old..new`).
  - Section 2: Asynchronously fetched diffs for every non-ignored changed flake
    input (GitHub Compare API via `curl`, optionally authenticated via
    `GITHUB_TOKEN`, `GH_TOKEN`, or `gh auth token`).
  - Pure version/hash/rev/checksum bump hunks are hidden by default with a
    per-input badge (`[f]` toggles showing/hiding them).
  - Inputs exceeding `max_diff_files` substantive files *after* filtering start
    collapsed with a commit summary and compare URL (`[e]` toggles expanding).
  - Navigation: `Tab` / `Left` / `Right` switch pane, `j`/`k`/arrows scroll,
    `PgUp`/`PgDn`/`Ctrl-d`/`Ctrl-u` page, `n`/`N` jump between input and file
    headers.
- **Right pane (`Build & Deploy`)**:
  - During `nix build`: live `internal-json` progress tree (elapsed,
    `done/expected`, running derivations + phase + log tail, active transfers,
    recent completions, warnings/errors). Quitting kills `nix build` cleanly.
  - After build: computes per-machine `current_system`, closure reboot verdict
    (structural symlinks `kernel`/`kernel-modules`/`initrd`/`systemd` +
    built-in patterns `nvidia-x11`/`linux-firmware` + per-machine
    `<toplevel>/deploy-reboot-patterns.json`), and captured `nvd diff` in a
    background thread.
  - **Activation gating**: If any reviewable flake input diffs exist, the right
    pane pauses at `[a]pprove & continue / [q] abort` so no machine switches
    before diffs are reviewed.
  - Per machine (tsugumi, then saya): shows `nvd diff` and reboot verdict;
    prompts `[s]witch / [b]oot / [r] boot+reboot / [q] abort`. Auto-switches
    only when there are no reviewable diffs AND no reboot-worthy changes.
  - Streams `nix copy` (8 parallel unmultiplexed ssh connections),
    `nix-env --set`, `switch-to-configuration`, and remote `reboot_and_wait`
    live into the right pane.
- **Abort rollback**: Aborting or failing after a `"Bumps"` commit was created
  or squashed prompts `[r]evert Bumps (jj op restore) / [k]eep Bumps`.
- **Push to master**:
  - Choosing `boot + reboot` on the local machine (`saya`) when unpushed
    commits exist prompts immediately before `systemctl reboot`:
    `[p]ush to master & reboot / [r]eboot without pushing / [c]ancel`.
  - Finishing all machines without a local reboot when unpushed commits exist
    prompts `[p]ush to master / [Enter/q] exit`.
- **Terminal safety**: An RAII guard + panic hook always restores raw mode and
  leaves the alternate screen; a concise exit summary (and failure details) is
  printed to stderr afterward.

## Plain / Non-TTY pipeline (v1 compatibility)

When stdin/stderr is not a TTY or `--mode` / `--check` / `--no-tui` is set:

1. `lineage` check against selected machines (`--check` exits here).
2. Single-evaluation `nix build --no-link --print-out-paths` (using `ui.rs`
   inline ANSI renderer when stderr is a TTY, or nix's own output otherwise).
3. Per machine: `diff::assess`, `diff::show_nvd_diff`, prompt or `--mode`,
   `activate::copy_closure`, `activate::activate`, and optional reboot.

## Crate layout

```
tools/nix-deploy/
  Cargo.toml          # anyhow, clap, crossterm, ratatui, regex, serde/serde_json, terminal_size, toml
  src/
    main.rs           # CLI flags, TUI vs plain dispatch, plain pipeline
    manifest.rs       # deploy.toml (machines + settings + diff defaults)
    repo.rs           # jj/git inspection, rebase, commit hygiene, Bumps, op restore, push
    flake_diff.rs     # speculative nix flake update, lockfile v7 diff, GitHub diffs, filtering
    sysmon.rs         # /proc/stat, /proc/meminfo, ZFS ARC, /proc/loadavg sparkline sampler
    build.rs          # single-eval nix build (plain + cancellable TUI BuildHandle)
    log_model.rs      # internal-json parser + build-state model
    ui.rs             # inline ANSI build progress display for plain TTY mode
    tui/
      mod.rs          # terminal RAII guard, panic hook, worker threads, event loop
      state.rs        # TUI state machine, diff flattening, filter/collapse/jump helpers
      render.rs       # ratatui widgets (sparklines, responsive panes, modals, footer)
    diff.rs           # closure listing, structural + pattern checks, nvd diff
    lineage.rs        # running vs. deployed revision, rollback refusal
    target.rs         # ssh/local command abstraction (capture, streamed, callback streaming)
    activate.rs       # profile set + switch-to-configuration + reboot flow (plain + streaming)
```

`check-rust-tools` covers the crate like every other tool. `agents/rust.md`
applies.
