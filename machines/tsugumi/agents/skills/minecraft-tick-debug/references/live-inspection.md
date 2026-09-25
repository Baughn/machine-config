# Read-only live inspection

Read the [builder's inspector documentation](/home/minecraft/builder/mods/live-inspector/README.md)
for the wish-list, complete interface, limits, semantic caveats and porting boundaries.
The installed adapter is for E36 / Minecraft 1.12.2 / Cleanroom 0.6.12-alpha;
query `status` to verify what the running server actually supports.

## Gather evidence without visiting the area

Use `scripts/evidence.py --server SERVER inspect 'QUERY' --out NEW_JSON_FILE`.
The parent directory must exist. This saves raw JSON locally and checks protocol
errors. All commands are console/RCON commands; clients need no mod. A missing
inspector command is a deployment gap, not permission to restart and destroy a
currently active incident's evidence.

Choose queries by question:

| Question | Query |
| --- | --- |
| What worlds/players/workload are present? | `status` |
| Where are resident entities/items concentrated? | `census` or `census DIM` |
| What is at this chunk without loading it? | `chunk DIM CX CZ [ENTITY_OFFSET]` |
| Which entity/tile update calls actually run, and which are expensive? | `watch start 30 [DIM CX CZ]`, then `watch status` after expiration |
| Where might newly appearing entities come from? | The same watch's `joins` records and call stacks |

A complete census gives per-chunk counts/classes and item units, not every entity's
UUID. Follow suspect coordinates with a chunk query. Chunk queries expose ordinary
entity identities and item-specific ages/lifespans, list membership, missing update
neighbors, unload/ticket metadata, and resident tile classes nearby. Never use
teleport, `/execute` in a missing chunk, chunk-loader changes, or a client spectator
visit merely to inspect an anomaly. A visit can change the lifecycle being observed.

Check `complete`, `details_complete`, category truncation, `observer_errors`,
`hooks_observed_since_start`, and dropped watch observations. A false hook flag
means unverified/unavailable coverage, not inactivity. A true flag proves that
path has run this session; other mod schedulers may still bypass it. E36 includes
an optional HammerLib adapter for its replaced tile-update path. A partial census
is a lower bound; narrow the query.
First-use class/JIT work can cause a soft-budget truncation, so one subsequent
narrowed query can be useful. Avoid repeatedly scanning an overloaded world.
Entity-page offsets are not stable cursors across ticks; preserve timestamps and
deduplicate UUIDs. Do not infer absence from a page or incomplete capture.
Global watches can include mod-managed ticking objects with unknown locations;
null dimensions/coordinates are deliberate. Scoped watches exclude these and
report `unlocated_scoped_observations`. Read bounded `observer_error_samples` when
an adapter fails; do not treat failed observations as zero work.

## Compare observations and form hypotheses

Take the same chunk or census query twice, separated by 10–60 seconds under the
same player activity. Save each response before discussing it. Run:

```sh
python3 scripts/compare_inspections.py BEFORE.json AFTER.json > COMPARISON.json
```

The helper rejects different sessions/queries and observations with no elapsed
server ticks. It compares resident counts only for complete scans, reports missing
UUIDs only for complete detail snapshots, and retains observed per-UUID age/count
changes even when broader membership cannot be established. A restart invalidates
the session comparison. Missing entities may have moved, merged, unloaded, expired,
or been collected; new UUIDs may be loads, not spawns.

The common non-aging-item case is a useful recipe, not the toolkit's sole purpose:
find growing item **entity counts and stack units**, then match retained UUIDs.
Unchanged item age **and** `ticks_existed` while server ticks advance suggests that
those objects are not getting regular updates. If entity ticks advance but age
does not, investigate item-specific logic, `update_blocked`, age resets/merging,
and lifespan/never-despawn settings. The `-32768` age sentinel is not a tick failure.

A failed neighbor prerequisite is only structural evidence: Forge hooks or other
mods may override it. A scoped watch records actual update calls at known call
sites. With complete coverage, a resident non-aging item that never appears in
updates, beside a tile that keeps updating, is stronger evidence of mismatched
chunk/entity activity. The observer does not run `canEntityUpdate` to test it,
because firing hooks could itself change behavior.

Generalize the same workflow to crowded mobs, stuck tile entities, repeated spawns,
chunk-boundary anomalies, and expensive machines. Rank watched total/max elapsed
update time and call counts alongside the sampled CPU profile. Timings are inclusive
wall time and can overlap; async work and other update call sites are outside
coverage. Location is first-observed location. Ticket ownership, a nearby tile,
and a join stack are clues, not proof of a particular admin/player's responsibility.

Watch joins are bounded event observations. Check origin hints/stack truncation;
load paths and cancelled events can look like spawns. Preserve the stack and
correlate the emitting class with the deployed mod/source before suggesting a
machine change. If exact provenance is absent, report it as unknown.

## Output and limits

Provide the saved evidence paths, affected dimension/chunk/block coordinates,
observed growth or hot update paths, confidence, competing explanations, and
specific suggestions for an administrator. Do not clear items, edit inventories,
force/unforce chunks, disable machines, or apply fixes. Keep the evidence local
unless publication is separately requested. The workflow ends with readouts and
suggestions; world mutations belong to a separate explicit administrative task.
