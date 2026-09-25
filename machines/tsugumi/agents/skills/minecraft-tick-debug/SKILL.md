---
name: minecraft-tick-debug
description: Collect and analyze tick-time evidence for Erisia Minecraft servers, including healthy baselines, low TPS, stalls, GC pressure, mod hotspots, and read-only investigation of resident entities, items, machines, and chunk activity. Use for server debugging, not client FPS or unrelated network lag.
---

# Minecraft tick debugging

Produce a reproducible local evidence bundle and a diagnosis tied to its timestamps, call paths, and workload. When the server is healthy, establish a baseline and identify collection gaps; do not invent a bottleneck. A skill invocation runs this workflow; it does not install a continuous monitoring service.

## Learn diagnostic ordering

Use the draft [adaptive diagnostic ordering protocol](references/diagnostic-ordering.md)
to choose follow-ups and record what helped. Store case records, raw artifacts,
and evolving ordering preferences in `~/agent-debugging`, outside the skill.
Consult relevant history without delaying urgent evidence collection; keep deployment
checks, collection prerequisites, and evidence semantics intact. History informs
priority, not certainty or permission. Update the external records as investigations
finish; revise this skill only for demonstrated procedural or tooling changes.

## Discover the actual deployment

Read [references/erisia.md](references/erisia.md) for the verified collection paths, profiler syntax, metrics, and fallbacks. Default to `~/erisia`; `~/incognito` is usually secondary. Build sources are in `~/builder`. Recheck the target file, deployed store path, mod JARs, and live Java executable each time. Repository notes can describe an older pack. Never infer the JVM PID from `server.pid` (the Python launcher), or assume `server-jvm.pid` exists under systemd.

## Collect

Use `scripts/evidence.py --server SERVER snapshot --out NEW_DIRECTORY` to capture metrics, console diagnostics, deployed JAR hashes, selected server settings, JVM flags, process/host counters, and bounded log tails. Paths are relative to this skill. Output directories are private and must not already exist. Exit 2 means a partial bundle: read `collection.json` and repair missing access or record the gap. Sandbox PID/network isolation is not evidence that the server is down; use the available host execution permission mechanism when needed.

For a baseline or incident, collect snapshots before and after a 30–120 second profile, retaining UTC timestamps and the user's description of player activity, exploration, machines, and recent changes. Compare player count, loaded dimensions/chunks, mod hashes, JVM version, and uptime. A new or empty-world baseline is not a prediction of mature-world performance. Longer incidents may need selected rotated logs; tails are deliberately bounded and may miss the incident.

For E36 with Flare, use the tested Java-sampler capture helper:

```sh
python3 scripts/evidence.py --server ~/erisia profile --seconds 30 --out NEW_CAPTURE_DIRECTORY
```

It checks for an existing sampler, uses a local lock against overlapping helper invocations, starts the Java sampler, explicitly stops/exports it after the requested duration, confirms cleanup, and copies the new profile into the bundle. A server-side timeout is also set as a sampling backstop. `--only-ticks-over 50` optionally restricts samples to slow ticks. Read `commands.json` when anything fails. Confirm that decoding yields samples; a copied file alone is not proof of usable data.

Flare 0.8.0 on this deployment has a verified trap: its timeout stops sampling but leaves the container active and does not export. The native backend also failed with a null `currentJob` during cleanup. Use the Java helper by default until an upgrade is tested. Never start a native sampler merely because the command reports success. For an interrupted capture, inspect `flare sampler info` before retrying; a lost RCON response can mean a command executed. Do not stop another operator's sampler. Use `flare sampler stop --save-to-file` only to clean up this invocation's known sampler.

Read [references/erisia.md](references/erisia.md) for slow-tick filtering, all-thread captures, decoding, and JFR fallbacks. Take one ordinary profile first; filtering only slow ticks is unsuitable for measuring a healthy baseline. Preserve the original profile alongside decoded tables and the exact command/backend used. No profile file or zero samples means collection failed or missed the event, not that a suspected mod is innocent.

## Inspect live Minecraft state

For spatial causes, growing/stuck entity populations, machine behavior, and chunk
activity, read [references/live-inspection.md](references/live-inspection.md).
Use the server-only inspector's bounded resident-world queries, temporal comparisons,
and passive update/join watches. Preserve evidence without visiting or loading the
suspect area. Item buildup is one common recipe; choose observations that answer
the broader question rather than assuming that particular failure mode.

## Analyze

- Use MSPT distributions and wall-clock TPS together. At the normal 20 TPS target the budget is 50 ms; 20 TPS can coexist with shrinking headroom or occasional long stalls. Different tools use different rolling windows; report the window. Inspect GC and host scheduling/I/O alongside tick time.
- Decode Flare with `scripts/ProfileSummary.java` as documented in the reference. Rank server-thread frames by self and inclusive weight, then follow their child references to reconstruct hot paths. Inclusive frames overlap; do not add ancestor and descendant costs. Sampling percentages are not exact milliseconds per game tick. Normal sleep/wait in an idle tick loop is expected. High machine-wide CPU or summed all-thread weights do not prove server-thread saturation.
- Correlate suspect call paths with Flare's source metadata, exact deployed mod JARs, and source/decompiled methods where necessary. Preserve obfuscated names and verify the mapping version. A mixin or a callee can be responsible even when the enclosing class belongs to another mod.
- Use dimensions, entity/tile-entity/chunk counts and worldgen counters to choose a narrower follow-up. Entity density is not measured entity cost. A stack trace usually identifies code, not an offending block's coordinates. Use the live inspector for resident object coordinates and bounded actual update observations. If source attribution or coverage is missing, report that gap explicitly.
- Compare baseline and incident under similar activity. If nothing is slow, record the observed baseline and stop. Otherwise identify the best-supported cause, plausible alternatives, and the smallest additional capture needed to distinguish them.
- If useful evidence is missing, consider ways of acquiring it. Do not alter the server unless directly asked to do so, but do suggest alterations to the inspection framework.

Keep raw logs/profiles local: they can contain player identifiers, coordinates, and configuration. Do not use upload/view commands as a substitute for local decoding. Do not restart, change JVM flags/mods, unload chunks, delete entities, or modify the world merely to diagnose it. This diagnostic workflow produces readouts and suggestions for administrators; it does not autonomously apply fixes.

Report the observed TPS/MSPT and workload, strongest evidence with artifact paths, likely cause and confidence, collection limitations, and the next concrete action. Do not post the report to Discord or other services unless separately instructed.
