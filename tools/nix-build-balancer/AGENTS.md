# Agent Instructions for nix-build-balancer

These instructions apply to `tools/nix-build-balancer`.

## Project Shape

`nix-build-balancer` is a small Rust crate with one `[lib]` and four
`[[bin]]` targets, used by the surrounding NixOS configuration:

- `nbb-controller` runs on the controller host. Maintains a TCP polling
  connection to every agent, holds the SQLite history, and answers
  `DECIDE_CANDIDATE` calls from the local `nbb-hook`.
- `nbb-agent` runs on every build host. Listens on TCP for the
  controller's polling connection, watches `/var/lib/nbb/spool/*.evt`,
  and forwards matched `EVENT_BUILD_FINISH` frames upstream. Supports
  `--once` for a single diagnostic telemetry print.
- `nbb-hook` runs on the controller host only. Implements Nix's
  build-hook stdin protocol; asks the local controller per candidate
  and delegates accepted builds to `nix __build-remote`.
- `nbb-event` runs on every build host. One-shot CLI invoked by Nix's
  `pre-build-hook` / `post-build-hook`; writes a single bincode
  `SpoolEvent` to `/var/lib/nbb/spool/<ulid>.evt`, fsyncs, and exits.

Read `SPEC.md` (current target) before making behavior changes. The
rewrite intentionally drops the prototype's `serve`/`telemetry`
single-binary shape and the axum-based HTTP API.

## Design Bias

Keep this tool boring and narrow.

- Prefer the standard library and existing dependencies when they are enough.
- The controller and agent use `tokio` (TCP, Unix socket, watchdog
  timer). `nbb-hook` and `nbb-event` are synchronous — Nix's build-hook
  protocol is stdin/stderr/child-process driven and benefits from no
  async runtime. Do not push tokio into either of those binaries.
- Do not add `anyhow` or `thiserror`. `io::Error` and the bincode error
  types cover what's needed.
- Preserve fail-closed behavior:
  - `nbb-event` must never fail a Nix build (any error → log to stderr,
    exit 0).
  - `nbb-hook` declines on any controller-side error.
- Keep dependencies minimal. Any new crate should solve a concrete
  problem better than a small local implementation.

## Rust Quality Rules

- Use `cargo` for build, test, lint, and formatting work.
- Keep `cargo fmt --check`, `cargo build`, `cargo clippy -- -D warnings`,
  and `cargo test` passing after Rust changes.
- Do not use `.unwrap()` in production code paths. Test code may use
  `.unwrap()` when the failure would make the test invalid.
- Use `.expect()` only for clear invariant violations, with a useful
  message.
- Avoid `unsafe`. The only places that need it today are the `flock`
  and `kill` libc calls; each has a `SAFETY:` comment.
- Keep comments sparse. Add comments for protocol details, safety
  invariants, or Rust-specific behavior that would not be obvious to a
  Python-fluent reader.

## Protocol and Output Constraints

Be careful with stdout and stderr:

- Nix build-hook directives such as `# decline` are protocol output and
  must remain on stderr. `nbb-hook` is synchronous so this is direct.
- Delegated `nix __build-remote` stderr is proxied to stderr by
  `nbb-hook`'s `delegate.rs`.
- Daemon diagnostics use `tracing` with `tracing-subscriber` writing to
  stderr. Filter via the `NBB_LOG` env var (defaults to `info`).
  systemd captures this.
- `nbb-event` writes any error to stderr (which Nix proxies away) and
  always exits 0.

The Nix hook wire protocol (`src/nix_protocol.rs`) is binary and
padding-sensitive. The custom controller/agent wire protocol
(`src/protocol/`) is also binary; both have round-trip tests.

## Wire Protocol Summary

- 6-byte frame header: `u16 op_id` + `u32 length`, little-endian; body
  up to 1 MiB; bincode bodies (`bincode::config::standard()`).
- First 32 bytes of every connection are the source-tree hash baked at
  compile time by `build.rs`. Bytewise compare; mismatch closes the
  connection immediately.
- Controller ↔ agent: long-lived TCP. PING/PONG, TELEMETRY_GET/TELEMETRY,
  unsolicited EVENT_BUILD_FINISH pushes.
- Hook ↔ controller: one-shot Unix socket. DECIDE_CANDIDATE/DECISION and
  ADMISSION_FINISH.
- Event submitter → agent: disk spool only (no socket).

## Scheduler Rules

One function in `src/scheduler.rs`. Deterministic:

- Drop wrong-system candidates.
- Drop targets whose last `PONG` is older than `poll_interval × 3` or
  whose `mem_available_kb` is below the configured floor.
- `package_ms = predict_ms(pname) × speed_multiplier` (fallback
  `unknown_p95_ms` when no observations exist). The estimator is the
  log-normal EWMA in `src/estimator.rs` — see SPEC §"Duration estimator"
  for the model, knobs, and references.
- `queue_ms = Σ admissions_for_target.predicted_ms / target.capacity`.
  Admissions are the **only** load signal. `nix_slots_active` is
  reported in telemetry for observability but never enters the formula.
- Pick the smallest `completion_ms = queue_ms + package_ms`. If the
  winner is the controller's own host, return `Decline`.

Do not introduce a learned model, random scheduling, global fairness
policy, exploration percent, or CPU-busy checks. They were removed
deliberately. The per-pname log-normal EWMA estimator
(`src/estimator.rs`) is closed-form statistics, not ML — do not
generalise it to a multi-feature or trained model; if you need that,
talk first.

## Persistence Rules

SQLite at `/var/lib/nbb/state.db`. Controller-only — agents have no DB.

- `build_observations(host, pname, drv_path, started_at_ms,
  finished_at_ms, duration_ms, status, out_paths)`. Unique on
  `(drv_path, finished_at_ms)` for idempotent duplicate-finish handling.
- `admissions(drv_path PRIMARY KEY, target_name, admitted_at_ms,
  predicted_ms)`. Cleared on controller startup.
- `meta(key, value)` — schema version.

When changing persistence, add tests for the migration or new query
behavior.

## Admission Lifecycle

Four retirement signals, all funneled through the same DB call. The
controller's 5-second watchdog is the time-driven floor:

1. `ADMISSION_FINISH` from the hook (happy path).
2. `EVENT_BUILD_FINISH` from the agent (writes an observation row when
   `duration_ms` is present; retires the admission either way).
3. Sentinel sweep: `/run/nbb/inflight/<drv_hash>` whose PID returns
   `ESRCH` from `kill(pid, 0)` is removed and the admission retired.
4. Wall-clock TTL: admissions older than
   `max(predicted_ms × 2, 60_000)` ms are swept.

## Module Layout

- `src/lib.rs` — re-exports plus the source-tree hash constants.
- `src/bin/{nbb_controller,nbb_agent,nbb_hook,nbb_event}.rs` — thin CLI
  entry points.
- `src/protocol/` — frame codec (`frame.rs`), handshake (`handshake.rs`),
  bincode op-body types (`ops.rs`).
- `src/agent/` — TCP server + spool watcher + telemetry sampler.
- `src/controller/` — TCP poller, Unix-socket hook server, 5s watchdog,
  scheduler entry.
- `src/hook/` — Nix build-hook driver, candidate codec, `nix
  __build-remote` delegation.
- `src/scheduler.rs` — `decide_build_candidate`.
- `src/estimator.rs` — log-normal EWMA duration estimator (math + tests).
- `src/persistence/{mod,observations,admissions}.rs` + `schema.sql`.
- `src/telemetry.rs` — procfs / PSI / flock slot counter.
- `src/nix_protocol.rs` — Nix wire-format helpers (8-byte-aligned).
- `src/inflight.rs` — sentinel writer / reader / PID-liveness check.
- `src/spool.rs` — atomic spool-file writer used by `nbb-event`.
- `src/util.rs` — pname normalization, time, hostname helpers.
- `tests/lifecycle.rs` — integration tests for the lifecycle invariants
  from SPEC §"Build observation lifecycle".

Tests live alongside the code they exercise via `#[cfg(test)] mod
tests`, plus the cross-module integration tests in `tests/`.

## Testing Expectations

For behavior changes, add or update focused unit tests. Highest-value
tests are scheduler decisions, Nix protocol round-trips, persistence
behavior, watchdog retirement paths, and `pname` normalization.

Before handing work back, run:

```sh
cargo fmt --check
cargo build
cargo clippy -- -D warnings
cargo test
```

If a command cannot be run, say which one and why.
