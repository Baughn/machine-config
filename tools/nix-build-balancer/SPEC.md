# nix-build-balancer — Rebuild Spec

This is the target spec for a rewrite of `nix-build-balancer`. It supersedes
the assumptions in `DESIGN.md`, which describes today's prototype. Keep
`DESIGN.md` in sync with the running code until this rewrite is merged, then
delete it.

The goal of this rewrite is a smaller codebase that does only what the
prototype proved useful, and removes the asymmetries and fragility that
emerged along the way.

The old codebase is in old-src. Only explore it if necessary, and use a sub-agent to do so.

## Scope and non-goals

In scope:

- One controller. N homogeneous agents (≥ 1). The controller's host may
  itself run an agent — there is no "localhost vs remote" code path.
- One Nix-system architecture per controller (today `x86_64-linux`).
  Agents that report a different system are ignored.
- A single estimate per `pname` rolled across all agents, optionally scaled by
  a per-agent performance multiplier (default 1.0; TODO until a slow agent
  exists).

Out of scope:

- Cross-architecture routing, feature-set matching, learned models, fairness
  policies, push-based telemetry, authentication beyond network trust + the
  source-tree handshake described below.

## Binaries

The crate ships four `[[bin]]` targets. They share a `lib` for protocol,
storage, and scheduler code.

| Binary           | Where it runs        | Role                                                                 |
|------------------|----------------------|----------------------------------------------------------------------|
| `nbb-controller` | one host (saya)      | Holds history DB, polls agents, decides build candidates.            |
| `nbb-agent`      | every build host     | Publishes local telemetry. Accepts event submissions from local Nix. |
| `nbb-hook`       | controller host only | Implements Nix build-hook protocol; asks controller per candidate.   |
| `nbb-event`      | every build host     | One-shot CLI invoked by Nix `pre-build-hook` / `post-build-hook`.    |

`nbb-event` is intentionally tiny: open the agent's Unix socket, write one
frame (start or finish), exit. No async runtime, no retries.

`nbb-hook` runs on the controller host because that's where the user invokes
`nixos-rebuild`. It speaks only to the local controller's Unix socket.

The current `telemetry` one-shot diagnostic CLI moves to `nbb-agent --once`.

## Wire protocol

Custom length-prefixed binary frames over TCP (controller ↔ agent) and Unix
seqpacket socket (hook/event ↔ local daemon). HTTP, axum, tower, hyper, and
the hand-rolled HTTP parser go away.

### Handshake

The first frame on every connection is a fixed 32-byte source-tree hash. The
hash is baked into the binary at build time via build.rs.
Peers compare bytewise. Mismatch closes the connection immediately with no
further bytes written, and an error logged. This replaces version negotiation because both ends
ship together through the flake.

### Frame format

```
+--------+--------+-------------------+
|  u16   |  u32   |       body        |
| op_id  | length |   length bytes    |
+--------+--------+-------------------+
```

- Multi-byte ints are little-endian.
- `length` caps at 1 MiB per frame; violators close the connection.
- Bodies use `bincode`.

### Operations (initial set)

Controller ↔ Agent:

- `AGENT_HELLO` — agent identifies itself (`name`, `system`, `capacity`).
- `TELEMETRY_GET` / `TELEMETRY` — controller pulls one snapshot.
- `EVENT_BUILD_FINISH` — push from agent to controller with
  `{drv_path, pname, host, ts_ms, duration_ms?, status}`. `duration_ms` is
  optional: when absent (e.g. agent restarted between start and finish), the
  controller still retires the matching admission but does not write an
  observation row. Build-start events do **not** cross the wire — they live
  only in the agent's memory until the matching finish arrives.
- `PING` / `PONG` — heartbeat. Used as a liveness substitute for the old
  `stale_telemetry_ms` rule.

Hook → Controller (Unix socket):

- `DECIDE_CANDIDATE` / `DECISION` — request returns
  `{action: Accept | Decline, target?: {name, store_uri, builder_line}}`.
- `ADMISSION_FINISH` — hook reports terminal status of a delegated build.

Event submitter → Agent: **disk spool, fire-and-forget**.

- `nbb-event` writes one bincode-encoded event frame to
  `/var/lib/nbb/spool/<ulid>.evt`, fsyncs, and exits. No socket, no
  timeout, no retry. If the write fails, log to stderr and exit 0 — Nix's
  pre/post-build-hook ignores the result anyway, and we must never block
  or fail a Nix build because of telemetry.
- The agent watches the spool directory (1 s poll is fine; inotify is an
  optional optimization). For each file: parse, apply locally (start →
  remember in-memory; finish → compute duration if a matching start exists,
  then forward to controller), unlink on success, leave in place on transient
  controller-unreachable so the next tick retries.
- The spool path is on persistent disk (not `/run`) so a brief agent
  outage does not lose events. Stale spool entries after a reboot are
  harmless: they refresh stats and retire any admissions the controller may
  still believe are active.

All in-process operations are one-shot: write request, read response, close.
No streaming, no long-lived sessions other than the controller's
`PING`-driven polling connection.

## Data model

```
Target {
  name: String,
  store_uri: String,
  builder_line: String,   // pre-formatted Nix machine line
  capacity: usize,
  speed_multiplier: f64,  // 1.0 today; TODO once a slow builder exists
  is_controller_host: bool, // for hook-side display only; no scheduler effect
}

Telemetry {
  mem_available_kb: u64,
  psi_memory_some_avg10: Option<f64>,
  nix_slots_active: usize, // count of locked slot files; not split local/remote
  sampled_at_ms: u128,
}

PackageStats {
  pname: String,
  count: u64,
  p95_ms: u64,           // single global estimate across all agents
}

Admission {
  drv_path: String,
  target_name: String,
  admitted_at_ms: u128,
  predicted_ms: u64,
}
```

Note: telemetry no longer carries `nix_slots_local` / `nix_slots_remote`. The
agent reports its own active slots. From the controller's view, all slots on
a given agent are "load on that agent".

### Persistence

SQLite stays. Reasons: cheap durability, queryable, already proven for the
history table. New schema is a subset of today's:

- `build_observations(host, pname, drv_path, started_at_ms, finished_at_ms,
   duration_ms, status, out_paths)` — one row per matched completion. Capped
  per `pname` like today.
- `admissions(drv_path PRIMARY KEY, target_name, admitted_at_ms,
   predicted_ms)` — controller-side.
- `meta(key, value)` — schema version.

`active_builds` (today's unmatched-start table) is dropped. We rely on the
event stream from agents to drive completion accounting; if a start is never
matched by a finish, the build observation is simply not recorded — the
controller doesn't need to track unmatched starts as queue load because slot
files already do.

Per-agent stats files (`telemetry-<host>.json`, `stats-<host>.json`) go
away. The controller holds everything in memory; the disk path is only
SQLite.

## Scheduler

For each candidate from the hook:

1. Drop if `system` != controller's configured system.
2. Take a fresh telemetry snapshot per target. Drop targets where the last
   `PONG` is older than the polling interval × 3 or where
   `mem_available_kb < min_remote_mem_available_kb`.
3. For each surviving target:
   - `package_ms = stats[pname].p95_ms` (single global estimate) ×
     `target.speed_multiplier`, falling back to `unknown_p95_ms` when no
     observations exist.
   - `queue_ms = (Σ admissions.predicted_ms) / capacity`. Admissions are the
     authoritative load signal because the controller knows exactly what it
     sent. The agent's `nix_slots_active` is reported in telemetry for
     observability but is **not** used in the formula — adding both
     double-counts every in-flight build and causes the kind of phantom-load
     spiral that pinned tsugumi at 16 builds in the prototype.
   - If `nix_slots_active` and `admissions.len()` for the same target
     diverge by more than 2 slots for longer than 30 s, log a warning. Do
     not act on it — investigate.
   - `completion_ms = queue_ms + package_ms`.
4. Pick the target with the smallest `completion_ms`. If it is the controller
   host's own agent, return `Decline` (let Nix build locally). Otherwise
   return `Accept{target}` and record an `Admission` row.

What is intentionally absent:

- No remote-CPU-busy-ratio check. Removed.
- No `max_unknown_remote`. Removed.
- No `min_remote_admission_interval_ms`. Removed.
- No exploration percent. Removed.
- No "borrow from other side" prediction hack. Removed by the single global
  estimate.
- No staleness window over wall-clock — replaced by `PING`/`PONG` liveness.

The result: one scheduler file, no policy struct knobs other than capacity,
memory backstop, and `unknown_p95_ms`.

## Build observation lifecycle

Invariants the spec requires; the implementation may choose the mechanism:

1. **At most one Admission row per drv_path.** Re-admission overwrites.
2. **Every Admission is eventually retired.** Retirement happens via one of:
   - hook reports `ADMISSION_FINISH` (success or failure) on its happy-path
     exit;
   - controller observes a `EVENT_BUILD_FINISH` for the same drv_path
     (forwarded by an agent draining its spool);
   - controller's watchdog detects a dead hook PID via the inflight sentinel
     (see below);
   - controller's watchdog wall-clock backstop: removes admissions older than
     `max(predicted_ms × 2, 60_000)` ms.
3. **The watchdog runs on a 5 s timer in the controller**, not just at hook
   decision time. The prototype only swept admissions at the top of
   `decide_build_candidate`, so a quiescent system never recovered: a
   crashed hook left an admission, no further hook calls fired, no sweep
   ran, and the slot count rotted. Time-driven retirement is the fix.
4. **Hook inflight sentinels.** When the hook accepts a candidate it writes
   `/run/nbb/inflight/<drv_hash>` containing
   `{pid, drv_path, admitted_at_ms, predicted_ms}` and unlinks it on every
   exit path (success, failure, signal-handler best effort). The watchdog
   walks this directory each tick; if a sentinel's PID is no longer alive
   (`kill(pid, 0) == ESRCH`), it synthesises a cancelled `ADMISSION_FINISH`
   immediately and unlinks the sentinel. This catches `nixos-rebuild` being
   killed mid-build, where the prototype quietly orphaned the admission.
5. **Build observation rows are written from `EVENT_BUILD_FINISH` only.**
   Cancellation paths (hook reports failure, watchdog retires, agent has no
   matching start so duration is absent) retire the Admission but do not
   write a build observation.
6. **Controller restart clears all Admissions.** A restart loses in-flight
   knowledge; agents will re-emit any new finishes from their on-disk spool.
7. **Hook crash mid-build:** the inflight-sentinel sweep retires within one
   watchdog tick (≤ 5 s). The wall-clock TTL is a long-stop in case the
   sentinel itself is missing.

This avoids the prototype's reliance on three overlapping mechanisms
(`active_builds`, `remote_admissions`, hook reporting). One table, four
retirement signals (happy-path hook report, agent finish event, sentinel
sweep, wall-clock backstop), all funneled through one timer-driven
watchdog.

## Test suite

The rewrite must ship with these tests. They exist to catch the failure
modes that bit us in the prototype.

**Protocol round-trips**
- Frame header encoder ↔ decoder symmetric.
- Source-tree-hash mismatch closes connection without further reads.
- Nix build-hook protocol (`read_nix_*` / `write_nix_*`) round-trips a `try`
  candidate including padding edges (0, 7, 8, 9 byte strings).

**Scheduler decisions**
- Empty history → all targets get `unknown_p95_ms`; pick stays deterministic
  by capacity and queue.
- One target memory-low → excluded; routing falls back to next-best.
- All targets memory-low → `Decline`.
- One target stale `PONG` → excluded.
- Single-target case (only controller host's agent) → always `Decline`.
- `speed_multiplier = 0.5` on one target → completion estimate halves.
- Admissions accumulate `queue_ms` correctly.

**Lifecycle (the previously brittle part)**
- Normal: start → admission recorded → finish → observation written,
  admission retired.
- Cancelled mid-build (hook returns failure, agent never emits finish):
  admission retired by hook report; no observation row.
- Hook crash (no `ADMISSION_FINISH`, no `EVENT_BUILD_FINISH`): inflight
  sentinel exists with dead PID; sentinel-sweep watchdog retires within one
  tick (≤ 5 s). No observation.
- Hook crash *and* sentinel missing (e.g. `/run` cleared mid-flight):
  wall-clock TTL retires after `max(predicted_ms × 2, 60_000)` ms.
- Agent restart mid-build: in-memory start state lost. The matching finish
  arrives without `duration_ms`; controller retires the admission but does
  not write an observation row. One lost stat sample is acceptable; a stuck
  slot is not.
- Controller restart with admissions in flight: SQLite admissions table
  cleared on startup; subsequent observations land cleanly.
- Duplicate `EVENT_BUILD_FINISH` (rare but possible): writes one observation,
  not two.
- Watchdog runs on a quiet system: with no hook calls happening, an admission
  whose hook process died is still retired within `max(predicted_ms × 2, 60s)`
  by the time-driven sweep, never `forever`.
- Spool replay after agent restart: events written before the restart are
  still picked up; finishes for builds the controller has already forgotten
  are no-ops (admission absent ⇒ retirement is a no-op; observation row is
  written if `duration_ms` is present).

**Stats**
- `pname` normalization unchanged from today; existing test cases preserved.
- Capping at `max_samples_per_pname` keeps the newest.
- `p95_ms` computed against the upper-bucket quantile.

Tests live alongside the module they exercise. Integration tests covering
the lifecycle invariants live in `tests/lifecycle.rs` and use an in-memory
SQLite plus a paired in-process controller + fake agent over a `tokio` duplex
stream.

## NixOS integration

`modules/nix-build-balancer.nix` shrinks:

- `me.nixBuildBalancer.role` is `controller`, `agent`, or `both` (kaho-style
  laptops would be `agent`-only when they arrive).
- `targets` becomes an attrset on the controller, each value carrying
  `storeUri`, `builderLine`, `capacity`, optional `speedMultiplier`.
- `installNixHooks` and `scheduler.enable` stay as toggles.
- The controller's own host name appears in `targets` if and only if it
  should be a routable build site. Today it always is; the option exists for
  laptops that should never build locally for power reasons.
- The source-tree hash is plumbed into the package via the Nix derivation,
  not configured by the module.

## Open TODOs (kept in code, not blocking the rewrite)

- Per-target `speed_multiplier` is currently always 1.0. Wire it through, but
  document that the value is untested until a heterogeneous builder lands.
- macOS / `aarch64-darwin` support arrives with kaho. The system filter is
  per-controller today, but the target struct already carries `system` so a
  future controller running on darwin can route within its own arch.
- Push-based telemetry (agent → controller stream instead of poll) is
  speculative. The current pull design is fine for two-host scale.
