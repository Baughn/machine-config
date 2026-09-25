# Erisia collection reference

Verified on 2026-09-17 against the deployment and installed JAR bytecode; recheck after upgrades.

## Deployment and access

- Primary: `/home/minecraft/erisia`; build checkout: `/home/minecraft/builder`.
- `server.nix-target`: `e36`. Minecraft 1.12.2, Cleanroom 0.6.12-alpha, Java 25, 8 GiB heap. Discover current Java from `/proc/PID/exe` with matching `/proc/PID/cwd` rather than using the shell's Java.
- Flare 0.8.0, LagGoggles FAT 1.12.2-4.9, Prometheus Integration 0.2.2 are deployed. A client connection's mod list in a log is not the server inventory.
- `server.pid` is the launcher PID. The Java process can be in a systemd scope, with no `server-jvm.pid`. The collector selects actual Java processes whose cwd matches the server; ambiguity must be resolved before attaching a JVM tool.
- `start.py` enables localhost RCON and rotates its password in `server.properties` on launch. Read it in-process; do not print it or pass it in command-line arguments. `scripts/evidence.py command` supports the listed diagnostic commands and bounded Flare execution profiles, assembles split RCON responses, and fails instead of falling back to blind console keystrokes.
- Existing `control.sh` has `check`, `players`, `say`, and `stop`, but no arbitrary command-response interface. `say` broadcasts to players. Existing `profile.sh` uses obsolete WarmRoast assumptions; do not use it for this workflow.

## Metrics

Read the port from `config/prometheus-integration.cfg`; E36 currently exposes `http://127.0.0.1:1224/metrics`. Inspect the real series names and TYPE/HELP lines. The following are this exporter, not similarly named modern exporters:

| Series | Interpretation |
| --- | --- |
| `ticks` | Cumulative server tick count; delta / elapsed seconds is achieved TPS |
| `tick_time` | Tick duration summary; values are seconds, multiply by 1000 for MSPT |
| `tick_time_sum`, `tick_time_count` | `1000 * delta(sum) / delta(count)` gives interval mean MSPT |
| `world_time{world=...,quantile=...}` | Per-world duration quantiles in seconds |
| `jvm_gc_collection_seconds_sum/count` | GC duration/count deltas by collector; concurrent GC time is not automatically stop-the-world pause time |
| `jvm_memory_bytes_used{area="heap"}` | Heap usage; a rising sawtooth alone is not a leak |
| `process_cpu_seconds_total` | Delta / elapsed seconds gives CPU cores used by the entire process |
| `chunks_loaded`, `worldgen`, `present` | Chunk load causes, generated chunks, player presence; inspect live labels/types before constructing queries |

Use timestamps around the scrapes, and verify `process_start_time_seconds` and JVM identity did not change. Counter resets invalidate naive deltas. Summary quantiles cannot be averaged or subtracted to produce interval quantiles; `NaN` is missing data, often for inactive dimensions, not zero cost. World percentiles cannot be summed into a server percentile. Snapshot files are point observations and counter baselines, not retained historical monitoring.

Useful PromQL with a correctly selected server/instance, if an existing Prometheus is available:

```promql
rate(ticks[1m])
1000 * rate(tick_time_sum[1m]) / rate(tick_time_count[1m])
1000 * tick_time{quantile="0.95"}
rate(process_cpu_seconds_total[1m])
rate(jvm_gc_collection_seconds_sum[1m])
```

The discovery here verified the exporter, not a Prometheus scrape/retention service. An unattended future trigger needs such a service or a separate bounded polling job; creating this skill does not enable it. If requested later, use sustained MSPT/low TPS plus a separate spike signal, exclude startup, and add a cooldown, one-profile-at-a-time guard, duration/storage limits, and report retention. Avoid triggering an agent on every isolated long tick.

## Flare captures

Console commands omit the leading slash. The installed version uses `flare sampler`, not `spark profiler`.

- Baseline/sustained lag: prefer `evidence.py profile --seconds 60 --out NEW_DIRECTORY` (default server thread, Java backend, explicit stop/export).
- Spikes: `flare sampler start --timeout 135 --only-ticks-over 50 --save-to-file --force-java-sampler`. Choose the threshold from observed durations; report the included tick count and observation window.
- Background work/contention: `flare sampler start --timeout 75 --thread * --save-to-file --force-java-sampler`. Quote the whole command in the shell so `*` is literal.
- The native backend failed during the 2026-09-17 live test (`AsyncProfilerJob.stop()` on null `currentJob`); the Java backend produced a valid profile. Use Java here until the native backend is separately verified. This is an observed failure, not a proven root-cause diagnosis.

Flare 0.8.0 accepts `--save-to-file` at start, but live tests showed that the timeout only stops sampling: it does not export or clear the active sampler container. Explicitly send `flare sampler stop --save-to-file` at the intended end of every manual capture. The `profile` helper does this automatically and uses a slightly longer server timeout as a backstop. Minimum accepted timeout is greater than 10 seconds; helper capture duration is capped at 120 seconds. The server asynchronously writes `config/flare/profiler/<timestamp>.sparkprofile`; an RCON response may precede the file and the final message may not reach the originating connection. Verify a new, stable, parseable file and its embedded timestamps. If export was delayed after the sampling timeout, top-level end time/tick count include that delay: use actual window timestamps/weights for the sampled interval. In the live test, 30 seconds of samples were exported after about 65 seconds; do not label that a 65-second sample. Do not assume Spark's current flags or timeout-upload behavior are interchangeable with Flare.

## Headless profile extraction

Use Java 17+ source-file execution with the exact Flare JAR that generated the profile. E36's running JDK includes the tools. Set `tick_java` from the validated Java executable and `tick_flare` from the deployed JAR path:

```sh
umask 077
"$tick_java" -cp "$tick_flare" scripts/ProfileSummary.java CAPTURE.sparkprofile NEW_OUTPUT_DIRECTORY
```

The helper uses Flare's bundled protobuf parser, so it needs no downloaded libraries, running game, or viewer service. It accepts uncompressed execution `.sparkprofile` files written by Flare 0.8.0; allocation profiles and other profiler formats need their own decoder. It writes:

- `metadata.txt`: recording interval in microseconds, included ticks, threshold, JVM/platform, ordered window IDs.
- `threads.tsv`, `frames.tsv`: thread roots and indexed call nodes, inclusive/self sampled milliseconds, source IDs, child references, and per-window weights. Node IDs and child references are scoped to each thread. Self weight is inclusive minus direct children; materially negative values require investigating format/aggregation, not silently clamping them.
- `windows.tsv`: time windows, TPS, median/max MSPT, workload counts, CPU observations. Window statistics can themselves contain rolling measurements rather than statistics computed solely from that window. Zero-valued proto3 scalar fields can be absent/unavailable; cross-check against other evidence.
- `sources.tsv`: source ID to mod name/version, when populated.
- `platform.txt`: platform statistics including world/entity information when collected. Missing source or coordinate data is a collection limitation, not an inferred result.

Follow roots and `children_refs` for call paths; the `children` protobuf array is a flattened node table, not nested children. Per-window weights align with `metadata.txt`'s ordered window IDs. Retain the original binary; tables are an analysis convenience. Empty thread sets or all-zero weights are not a useful profile.

Source attribution and reference integrity can be checked against the JAR's `flare/flare.proto`, `flare/flare_sampler.proto`, `ProtoUtil`, and `ProtoTimeEncoder`. Source links: [Flare project](https://github.com/CleanroomMC/Flare), [start-command implementation](https://github.com/CleanroomMC/Flare/blob/master/src/main/java/com/cleanroommc/flare/common/command/sub/sampler/sub/SamplerStartCommand.java). The installed bytecode takes precedence over a moving upstream branch.

## Narrower follow-ups

For GC pauses, locks, native I/O, or an unresponsive console, identify the same Java PID/cwd/start time again and use its matching `jcmd`. Query `jcmd PID help JFR.start` and `JFR.check` first. A bounded `JFR.start name=tickdebug settings=profile duration=60s filename=ABSOLUTE_PATH` is a useful JVM-level fallback; choose an unused recording name and inspect supported settings. Decode locally with the JDK's `jfr summary` and `jfr print --json` with selected events. JFR alone does not supply modded per-dimension ticks or block coordinates. Query `Thread.print -l` for a suspected stall, recognizing that one dump is an instantaneous sample. Do not initiate full heap dumps or forced GC as routine tick diagnostics.

The Erisia live inspector supplies server-only, headless resident-world queries and bounded entity/tile update observations; read [live-inspection.md](live-inspection.md). LagGoggles remains an optional client UI, not a dependency. Saved NBT/entity counts describe persisted state rather than current residency/ticking; use the inspector before considering offline world scans.

JVM command references: [JDK 25 jcmd](https://docs.oracle.com/en/java/javase/25/docs/specs/man/jcmd.html), [JDK 25 jfr](https://docs.oracle.com/en/java/javase/25/docs/specs/man/jfr.html).
