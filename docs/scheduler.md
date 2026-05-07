# Nix Build Balancer Plan

## Summary

Build a two-phase replacement for vanilla Nix remote-build scheduling. Phase 1
adds an observation daemon on saya and tsugumi that records build durations,
current CPU/memory state, and active Nix build slots without changing
scheduling. Phase 2 adds a custom Nix build hook that asks the daemon whether
each candidate derivation should go remote or fall back locally.

The first implementation is Phase 1 only. Slurm is intentionally skipped.

## Phase 1: Observation Daemon

- Add a Rust daemon package, `tools/nix-build-balancer`, packaged through the
  existing crane overlay.
- Add a NixOS module, `modules/nix-build-balancer.nix`, with options under
  `me.nixBuildBalancer`.
- Run the daemon on both saya and tsugumi:
  - saya runs in `controller` mode and stores history locally.
  - tsugumi runs in `agent` mode and exposes telemetry to saya over WireGuard.
- Collect telemetry:
  - CPU idle/busy from `/proc/stat`.
  - memory availability from `/proc/meminfo`.
  - PSI memory pressure when available.
  - active Nix local/remote slots from `/nix/var/nix/current-load`.
  - build start/finish events from Nix `pre-build-hook` and `post-build-hook`.
- Store build history in SQLite, with scheduler-facing duration history keyed
  by normalized `pname` only.
- Retain a bounded number of recent completed observations per `pname`, so
  common packages cannot evict all history for rare packages.
- Derive initial predictions from rolling per-`pname` quantiles: p50, p80,
  p95, and sample count.
- Unknown `pname` gets a conservative default p95 so unknown builds cannot be
  treated as trivial.

## Daemon Interfaces

- Local Unix socket API on each host:
  - `GET /health`
  - `GET /telemetry`
  - `GET /stats`
  - `POST /event/build-start`
  - `POST /event/build-finish`
  - `POST /decision/build-candidate` reserved for Phase 2
- Controller polls remote agent telemetry every 1s by default.
- Hook/event payload includes:
  - drv path
  - output name
  - normalized `pname`
  - host
  - start/end timestamps
  - status: success, failure, cancelled, unknown
- Failures are non-fatal:
  - If daemon is unavailable, Nix hooks exit successfully and only skip
    observation.
  - Phase 2 scheduler will fail closed by declining remote admission.

## Phase 1 Storage

- Use `${dataDir}/history.sqlite3` as the only durable observation store.
- Track active starts in an `active_builds` table keyed by `drv_path`.
- Track matched completions in `build_observations`, including host, normalized
  `pname`, drv path, start/finish timestamps, duration, status, and output
  paths.
- `/stats` reads successful `build_observations` only and returns the
  `pname`-to-duration-quantile mapping needed by the scheduler.
- Do not migrate historical `events.tsv` or `builds.tsv`; Phase 1 can start a
  fresh database.

## Phase 2: Scheduler Hook

- Add a custom build hook that replaces `nix __build-remote`.
- For each candidate derivation, ask saya's daemon for a decision.
- Accept remote only when tsugumi has the shorter predicted queue:
  - Estimate local and remote queue length from active/admitted builds and their
    predicted remaining p95 durations.
  - Admit to tsugumi if predicted remote completion is shorter than predicted
    local completion.
  - Reject remote if tsugumi CPU/memory pressure is high, telemetry is stale,
    SSH is unhealthy, or prediction confidence is too low.
- No fixed queue-length target. The queue target is whichever machine is
  predicted to drain sooner.
- Keep hard safety bounds:
  - max remote admitted jobs
  - max unknown remote jobs
  - minimum interval between remote admissions
  - stale telemetry timeout
- Return `ssh-ng://svein@tsugumi.local` for accepted remote builds.
- Return `# decline` for local fallback.

## Model

- Start with deterministic rolling quantiles, not a random forest.
- Add a random forest or quantile-regression model only after Phase 1 has
  enough observations to compare against the quantile baseline.
- If using a random forest later, use it for build-duration distribution only.
  Queue prediction remains arithmetic over admitted/running jobs.
- Confidence policy:
  - High sample count and low recent error allows low p95 for trivial builders.
  - Low sample count, stale history, or high residual error uses conservative
    p95.
  - Unknown packages should prefer local unless remote is clearly idle.

## Test Plan

- Unit-test `pname` normalization against typical Nix output names, including
  KDE packages and local overlays.
- Unit-test telemetry parsing for `/proc/stat`, `/proc/meminfo`, PSI, and
  `/nix/var/nix/current-load`.
- Integration-test daemon start/stop and Unix socket APIs.
- Run Phase 1 during normal rebuilds and verify:
  - build events are recorded
  - tsugumi telemetry is visible from saya
  - per-`pname` duration quantiles update after builds
  - hook failures do not break Nix builds
- Before Phase 2, replay recorded histories through the decision engine and
  compare predicted vs actual queue drain times.
- Phase 2 acceptance test:
  - launch multiple uncached independent builds
  - verify both machines are used
  - verify unknown or high-uncertainty builds do not flood tsugumi
  - simulate daemon/SSH failure and confirm builds fall back locally

## Assumptions

- Phase 1 should land and run long enough to gather real data before any
  scheduling change.
- `pname` is the only durable history key for v1.
- Source tree size is excluded unless it is already available from cheap
  derivation/store metadata.
- This system optimizes CPU utilization across saya and tsugumi, not perfect
  fairness or global optimality.
- The scheduler hook can only choose remote or decline at admission time; it
  cannot reschedule a build after Nix starts it.
