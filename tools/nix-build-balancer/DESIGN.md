# Nix Build Balancer Design

## Purpose

`nix-build-balancer` is a small Nix build-hook helper for the current saya and
tsugumi setup. It observes local build history, polls one remote builder, and
decides whether each build-hook candidate should run locally or be delegated to
tsugumi.

The tool is deliberately single-purpose:

- saya runs the controller daemon and scheduler hook.
- tsugumi runs an agent daemon that exposes telemetry and package statistics.
- the hook either accepts exactly one configured remote builder or declines so
  Nix continues locally.

This is not a general cluster scheduler.

## Processes

The binary has four subcommands:

- `serve`: run the daemon. In `agent` mode it serves local telemetry and stats.
  In `controller` mode it can also poll remote agents and cache their latest
  telemetry and stats in the controller data directory.
- `event`: submit build start and finish events from Nix pre/post build hooks.
- `hook`: implement the Nix build-hook protocol, ask the local daemon for a
  decision, and invoke `nix __build-remote` only for accepted remote builds.
- `telemetry`: print a one-shot telemetry JSON payload for diagnostics.

The daemon serves HTTP over Unix sockets and/or TCP through `axum` on a
multi-threaded `tokio` runtime. The TCP listener is only for trusted remote
telemetry polling over the private network. The Unix socket carries
`event`, `hook`, and (planned) TUI traffic from local processes. Routes,
request/response types, and the synchronous client used by the CLI
subcommands all live under `src/api/` so a future TUI binary can depend on
the same wire contract.

The daemon was originally a small hand-rolled HTTP parser. It moved to axum
when a real external API consumer (the planned TUI) entered scope; before
that, the API was only consumed by the binary's own subcommands and a
framework was unnecessary.

## NixOS Integration

`modules/nix-build-balancer.nix` wires the binary into NixOS:

- `me.nixBuildBalancer.enable` installs and runs the daemon.
- `installNixHooks` installs best-effort observation hooks.
- `scheduler.enable` installs the custom build hook.
- `remoteAgents` lists agent addresses the controller polls.
- `scheduler.remoteHost`, `scheduler.remoteStoreUri`, and
  `scheduler.remoteBuilder` describe the single supported remote target.

The hooks are fail-closed. If the daemon is unavailable, the observation hooks
skip recording and the scheduler hook declines remote admission.

## Persistent State

The daemon stores durable local history in `${dataDir}/history.sqlite3`.

Tables:

- `active_builds`: unmatched local build starts keyed by `drv_path`.
- `build_observations`: matched build completions with host, normalized `pname`,
  derivation path, timestamps, duration, status, and output paths.
- `remote_admissions`: controller-side records for builds admitted to a remote
  host but not yet reported complete.
- `meta`: schema metadata.

Successful observations are retained per normalized `pname`, capped by
`--max-samples-per-pname`. Failed builds stay in history but do not contribute
to package-duration stats.

Controller polling writes the latest remote snapshots as:

- `telemetry-<host>.json`
- `stats-<host>.json`

These files are cache inputs to scheduling, not durable history.

## Telemetry

Each daemon samples:

- CPU busy ratio from `/proc/stat` through `procfs`.
- memory total and available values from `/proc/meminfo`.
- PSI memory pressure from `/proc/pressure/memory` when available.
- active Nix slot files from `/nix/var/nix/current-load`.

Slot files are counted only when they appear locked. The implementation uses
`flock` for this because Nix exposes slot activity through advisory locks.

## Scheduler Model

The scheduler uses deterministic rolling quantiles, not a learned model.

For each candidate derivation:

1. Normalize the derivation path to a package name.
2. Load local host state from live telemetry and SQLite history.
3. Load remote host state from cached telemetry, cached stats, and active
   admissions.
4. Reject incompatible candidates. Today only `x86_64-linux` and `builtin` are
   accepted.
5. Reject remote execution when telemetry is stale, CPU is busy, memory pressure
   is high, or available memory is low.
6. Predict local and remote package duration from p95 history. If only one side
   has samples, the other side borrows that package estimate. If neither side
   has samples, both use the conservative unknown default.
7. Estimate queue delay from active local builds, observed Nix slots, and remote
   admissions.
8. Apply remote admission safety limits.
9. Accept remote only when remote predicted completion beats local predicted
   completion, except for bounded exploration of empty hosts.
10. Record a remote admission only for accepted remote decisions.

The important scheduler types are:

- `SchedulerConfig`: local host, single remote target, and policy thresholds.
- `BuildTarget`: remote host name, store URI, and capacity.
- `HostState`: telemetry, package stats, local active work, and remote
  admissions.
- `Prediction`: package duration, queue delay, completion estimate, and sample
  count.
- `Eligibility`: accepted or declined with a stable reason.

## Hook Behavior

The hook reads Nix build-hook settings and `try` candidates from stdin. For each
candidate it posts a JSON request to `/decision/build-candidate`.

On decline, it prints `# decline` and waits for the next candidate. On accept,
it starts `nix __build-remote`, overrides the child `builders` setting with the
single configured builder line, forwards the candidate, then proxies the rest of
the Nix protocol after the child accepts.

When the remote build finishes or is cancelled, the hook reports
`/event/admission-finish` so the controller can remove the active remote
admission.

## Logging and Output

The binary intentionally uses stdout and stderr directly:

- CLI JSON output is written to stdout.
- Nix build-hook directives and delegated `nix __build-remote` output are
  written to stderr because that is the protocol surface Nix expects.
- The daemon uses `tracing` + `tracing-subscriber`, formatted to stderr and
  captured by systemd. Filtering uses the `NBB_LOG` env var (default `info`).
  `tower-http`'s `TraceLayer` records each request, and the polling task,
  listener startup, and shutdown signal go through `tracing` as well.

Hook diagnostics from `# decline` and the delegated `nix __build-remote`
child still go directly to stderr because they are protocol output.

## Current Limits

- Only one remote build target is supported.
- Required Nix system/features are not matched against a configured target set.
- Remote telemetry is cached by polling; there is no push protocol.
- Admission accounting is best-effort and tied to hook completion reporting.
- The HTTP surface assumes trusted callers (Unix socket peers and the
  controller's private-network poll). There is no auth on the listeners.
- SQLite calls run inside `tokio::task::spawn_blocking`; there is no async
  driver and no connection pool. Each handler opens a fresh connection.

## Future Options

These are speculative. They should not be treated as supported behavior until a
second real builder exists to test against.

### Multiple Remote Builders

Replace the single remote options with a target set:

```nix
me.nixBuildBalancer.scheduler.targets = {
  tsugumi = {
    storeUri = "ssh-ng://svein@tsugumi.local";
    builderLine = "ssh-ng://svein@tsugumi.local x86_64-linux /home/svein/.ssh/id_ed25519 16 1 nixos-test,kvm,big-parallel - -";
    capacity = 16;
    systems = [ "x86_64-linux" ];
    supportedFeatures = [ "nixos-test" "kvm" "big-parallel" ];
  };
};
```

The daemon would score each eligible target and return the selected host. The
hook would then pass only that target's builder line to `nix __build-remote`.
Passing all builders to Nix would make queue prediction and admission tracking
unreliable because Nix could choose a different host from the one scored by the
balancer.

### Richer Prediction

A future model could compare richer features or use a learned duration
distribution, but only after the deterministic p95 baseline has enough history
to evaluate against. Queue prediction should remain arithmetic over active and
admitted work unless real data shows that is insufficient.

### TUI Frontend

A read-only TUI is planned. It will subscribe to the daemon for currently
running builds and timeline expectations. Implementation will add new
endpoints (e.g. `/builds/active`, `/builds/timeline`) under `src/api/paths.rs`
and the corresponding response types under `src/api/types.rs`, then route
handlers in `src/daemon/routes.rs`. The TUI binary itself will live as a
sibling `[[bin]]` target in this crate so it shares the wire-types contract.
