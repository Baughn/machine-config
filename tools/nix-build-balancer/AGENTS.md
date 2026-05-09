# Agent Instructions for nix-build-balancer

These instructions apply to `tools/nix-build-balancer`.

## Project Shape

`nix-build-balancer` is a small Rust binary used by the surrounding NixOS
configuration. It is not a general service framework:

- `serve` runs the telemetry/statistics daemon.
- `event` submits Nix build start/finish observations.
- `hook` implements the Nix build-hook protocol and delegates to
  `nix __build-remote` only when the local daemon accepts a candidate.
- `telemetry` prints one diagnostic telemetry payload.

Read `DESIGN.md` before making behavior changes. Keep it updated when scheduler
behavior, persistence, hook protocol behavior, or NixOS integration changes.
Unsupported ideas belong under `Future Options`, not in the current behavior
sections.

## Design Bias

Keep this tool boring and narrow.

- Prefer the standard library and existing dependencies when they are enough.
- The `serve` daemon uses `tokio` + `axum` + `tower` + `tower-http` + `tracing`.
  This was added when a TUI consumer entered the roadmap, making the API a
  real external surface. CLI subcommands (`event`, `hook`, `telemetry`) stay
  synchronous on purpose: the build-hook is stdin/stderr/child-process driven
  and benefits from no async runtime. Do not push tokio into the hook path.
- Do not add `anyhow` or `thiserror`. The local `daemon::AppError` plus
  `io::Error` covers the daemon's needs without the macro surface.
- Preserve fail-closed behavior: daemon or telemetry failures should make the
  scheduler decline remote execution and let Nix continue locally. The hook
  client treats any non-200 response as decline.
- Preserve the single-remote model unless the user explicitly asks for
  multi-builder support and there is a real target to test against.
- Keep dependencies minimal. Any new crate should solve a concrete problem
  better than a small local implementation.

## Rust Quality Rules

- Use `cargo` for build, test, lint, and formatting work.
- Keep `cargo fmt --check`, `cargo build`, `cargo clippy -- -D warnings`, and
  `cargo test` passing after Rust changes.
- Do not use `.unwrap()` in production code paths. Test code may use `.unwrap()`
  when the failure would make the test invalid.
- Use `.expect()` only for clear invariant violations, with a useful message.
- Avoid `unsafe`. When it is necessary, add a short `SAFETY:` comment explaining
  the invariant.
- Keep functions focused and names descriptive.
- Prefer borrowing over cloning when it keeps the code simple.
- Avoid wildcard imports except `use super::*` inside tests.
- Keep comments sparse. Add comments for protocol details, safety invariants,
  or Rust-specific behavior that would not be obvious to a Python-fluent reader.

## Protocol and Output Constraints

Be careful with stdout and stderr:

- CLI JSON output belongs on stdout.
- Nix build-hook directives such as `# decline` are protocol output and must
  remain on stderr. The `hook` subcommand stays synchronous so this contract
  is direct.
- Delegated `nix __build-remote` stderr should continue to be proxied to stderr.
- Daemon diagnostics use `tracing` with the `tracing-subscriber` formatter
  writing to stderr. Filter via the `NBB_LOG` env var (defaults to `info`).
  Systemd captures this as before.

The Nix hook wire protocol is binary and padding-sensitive. Changes to
`read_nix_*` or `write_nix_*` helpers in `src/nix_protocol.rs` need focused
tests.

## Scheduler Rules

The current scheduler is deterministic:

- package duration estimates come from rolling p95 history by normalized
  `pname`;
- unknown packages use a conservative default;
- local and remote queue estimates are arithmetic over observed slots, active
  local builds, and admitted remote builds;
- remote execution is rejected on stale telemetry, busy CPU, high memory
  pressure, low memory, or admission-limit violations;
- bounded exploration is allowed only through the existing stable hash policy.

Do not introduce a learned model, random scheduling, global fairness policy, or
multi-host target selection unless explicitly requested.

## Persistence Rules

SQLite is the durable state store. Keep schema changes rare and intentional.

- `active_builds` tracks unmatched local starts.
- `build_observations` tracks matched completions.
- `remote_admissions` tracks accepted remote candidates until completion.
- `telemetry-<host>.json` and `stats-<host>.json` are controller-side caches,
  not durable history.

When changing persistence, add tests for the migration or new query behavior.

## Module Layout

`src/main.rs` is a thin dispatcher. Code lives under:

- `api/` — wire types (`BuildEvent`, `BuildCandidate`, `Decision`, …),
  `paths::*` route constants, and the synchronous `client` used by `event`,
  `hook`, and the controller's poller. This is the contract the future TUI
  binary will share.
- `cli.rs` — clap argument structs and `serve_config`.
- `config.rs` — `Config`, `Mode`, defaults shared between subcommands.
- `daemon/` — `serve` entry, `AppState`, axum `Router`, listeners, polling
  task, `AppError`. SQLite-touching handlers go through `spawn_blocking`.
- `hook/` — Nix build-hook subcommand: candidate codec, child delegation.
- `nix_protocol.rs` — Nix wire-format helpers.
- `persistence/` — SQLite schema, event recording, queries, cleanup.
- `scheduler/` — decision pipeline (`policy`, `state`, `eligibility`,
  `host_state`, plus `decide_build_candidate`).
- `telemetry/` — local procfs/flock collectors and remote cache reader/poller.
- `util.rs` — pname normalization, hostname, time, error converters.
- `test_support.rs` — `#[cfg(test)]` fixtures shared across tests.

Tests live alongside the code they exercise via `#[cfg(test)] mod tests`.

## Testing Expectations

For behavior changes, add or update focused unit tests in the same module as
the code under change. The highest-value tests are scheduler decisions, Nix
protocol round-trips, persistence behavior, and `pname` normalization.

Before handing work back, run:

```sh
cargo fmt --check
cargo build
cargo clippy -- -D warnings
cargo test
```

If a command cannot be run, say which one and why.
