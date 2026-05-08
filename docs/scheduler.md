# Nix Build Balancer Plan

## Summary

`nix-build-balancer` now has the core pieces in place: telemetry collection,
local history, remote polling, admission tracking, and a custom Nix build hook
that chooses between local execution and one remote builder.

The next useful work is not more scheduling policy. It is making the existing
single-remote scheduler easier to understand and change. The current decision
path is too procedural: host health checks, prediction lookup, queue math,
admission limits, exploration, admission recording, and response construction
are interleaved in one flow. That makes small policy changes riskier than they
should be.

This plan has two phases:

1. Clean up the current single-remote scheduler without changing behavior.
2. Sketch multi-build-host support in case a second remote builder becomes
   useful later.

## Phase 1: Clean Up Existing Logic

### Goal

Keep the current behavior, but make the scheduler read as a pipeline over
explicit data:

1. Parse the candidate.
2. Load local and remote host state.
3. Evaluate host eligibility.
4. Predict local and remote completion times.
5. Apply safety and exploration policy.
6. Emit a decision and record admission only for accepted remote builds.

The main output should be code that is easier to inspect and test, not a smarter
scheduler.

### Current Problems

- `decide_build_candidate` mixes data loading, policy checks, queue estimation,
  decision construction, and persistence.
- Several constants are global even though they describe a host or a scheduling
  policy.
- Remote state is represented indirectly by files named from the host instead
  of an explicit `HostState`.
- Queue estimates are built inline, so it is hard to tell which fields are
  observations and which are derived predictions.
- Decline reasons are useful, but they are emitted at different abstraction
  levels: telemetry freshness, memory pressure, admission limits, and queue
  comparisons all live side by side.
- The hook and daemon both know about the remote host/store/builder split, but
  there is no single type representing a scheduler target.

### Target Shape

Introduce small data types that make the decision declarative:

- `SchedulerConfig`
  - local host name
  - remote target
  - policy thresholds
  - capacity defaults
- `BuildTarget`
  - host name
  - store URI
  - Nix builder line
  - systems/features
  - capacity
- `HostState`
  - telemetry
  - package stats for the candidate
  - active/admitted builds
  - derived health status
- `Prediction`
  - package duration prediction
  - existing queue estimate
  - predicted completion time
  - sample counts and unknown flag
- `Eligibility`
  - accepted or declined
  - reason
  - metrics useful for logs/tests

The decision function should become mostly orchestration:

```text
candidate
  -> load_scheduler_state
  -> evaluate_local
  -> evaluate_remote
  -> compare_predictions
  -> apply_exploration_policy
  -> record_admission_if_needed
  -> decision
```

### Concrete Work

- Split `decide_build_candidate` into focused helpers:
  - load local/remote scheduler state
  - check candidate compatibility
  - check remote health
  - compute local prediction
  - compute remote prediction
  - apply admission limits
  - compare local vs remote
  - apply exploration policy
- Move policy constants into a policy struct passed through decision code.
- Replace ad hoc remote fields with a single target struct, even while there is
  only one remote target.
- Keep the existing HTTP API stable unless a small response field makes tests or
  logs materially clearer.
- Preserve current fail-closed behavior: daemon or telemetry failure means the
  hook declines and lets Nix continue locally.
- Keep persistence unchanged unless a small schema addition directly supports
  the cleanup.

### Tests

- Keep existing behavior tests passing.
- Add focused tests around the extracted pieces:
  - stale telemetry rejects remote
  - busy CPU rejects remote
  - low memory rejects remote
  - remote admission limit rejects remote
  - unknown remote admission limit rejects remote
  - local-vs-remote queue comparison chooses the expected side
  - exploration can still choose an empty remote host
  - admission is recorded only when the decision accepts remote
- Add one high-level regression test for the full single-remote decision path so
  the refactor has a behavioral backstop.

### Acceptance Criteria

- No scheduling behavior change.
- `decide_build_candidate` is short enough to read as a policy pipeline.
- Host health, prediction, admission limits, and final comparison are separately
  testable.
- The single remote target is represented by a type that can naturally become a
  list later.

## Phase 2: Potential Multi-Build-Host Design

This phase is optional. It only becomes useful once there is a second real
remote builder to configure and observe.

### Goal

Extend the scheduler from choosing `local` vs `one remote` to choosing `local`
vs `best eligible remote`.

The hook protocol can support this. The hook should ask the daemon for a
decision, and the daemon can return the selected remote target. The hook should
then pass exactly that target's builder line to `nix __build-remote`.

Passing all remotes to `__build-remote` would let Nix choose a different host
from the one the balancer scored, which would make admission tracking and queue
prediction unreliable.

### Configuration Shape

Use an explicit scheduler target set rather than parallel single-value options:

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

This can initially be generated from the existing single-remote options for
backwards compatibility, or the old options can be replaced in one migration.

### Decision Model

For each candidate:

1. Build a local prediction.
2. Iterate over configured remote targets.
3. Filter out remotes that are incompatible:
   - unsupported system
   - missing required features
   - stale telemetry
   - busy CPU
   - high memory pressure
   - low available memory
   - admission limit reached
4. Compute predicted completion time for each remaining remote.
5. Select the remote with the lowest predicted completion time.
6. Accept remote only if the selected remote beats local, subject to exploration
   policy.
7. Record admission against the selected host.

The decision response should include the selected host and store URI:

```json
{
  "decision": "accept",
  "remote_host": "tsugumi",
  "store_uri": "ssh-ng://svein@tsugumi.local",
  "reason": "remote predicted 120000ms vs local 180000ms"
}
```

### State and Persistence

The current `remote_admissions` table already includes `host`, so per-host
admission tracking is close to what multi-host scheduling needs.

Likely changes:

- keep telemetry and stats files per remote host
- keep admission limits per host
- add per-target capacity instead of using one global remote capacity
- include selected host in scheduler logs
- make stale admission cleanup host-agnostic, as it is today

### Hook Behavior

The hook should hold a map of configured targets. On accept:

1. Read `remote_host` from the daemon decision.
2. Look up that target's builder line.
3. Invoke `nix __build-remote`.
4. Override the `builders` setting with only the selected builder line.
5. Report admission finish for the selected derivation as it does today.

If the daemon returns an unknown host, the hook should decline rather than risk
delegating to the wrong builder.

### Tests

- Two healthy remotes: choose the faster predicted completion.
- One stale remote and one healthy remote: ignore stale and choose healthy.
- One busy remote and one idle remote: ignore busy and choose idle.
- Per-host admission limits do not block unrelated hosts.
- Required features filter targets before queue comparison.
- The hook passes only the selected target's builder line to `__build-remote`.
- Unknown selected host from the daemon causes local fallback.

### Non-Goals

- No global cluster scheduler.
- No attempt to reschedule a build after Nix starts it.
- No multi-hop delegation.
- No model change beyond comparing predicted completion times per host.
- No random forest or learned model until the simple deterministic scheduler is
  easy to reason about and has enough data to evaluate alternatives.
