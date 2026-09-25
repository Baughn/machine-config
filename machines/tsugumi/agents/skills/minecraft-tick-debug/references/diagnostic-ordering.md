# Adaptive diagnostic ordering — draft

Learn which debugging approach is useful next for the observed symptoms and
deployment. An unsuccessful attempt may reduce its priority in similar conditions;
it does not establish that the approach will fail next time. Keep diagnostic
ordering separate from beliefs about the underlying cause.

This adapts the historical-feedback idea from
[Dream-RSI](https://arxiv.org/html/2609.14858v1). It is initially a case-based
ordering procedure, not a validated replay simulator or automatic skill optimizer.

## Storage and retrieval

Keep evolving material in `/home/minecraft/agent-debugging`:

- `index.md`: one brief entry per case, with symptoms, deployment, outcome, and link.
- `ordering.md`: context-specific preferences, supporting and conflicting cases,
  prerequisites, and conditions for reconsideration.
- `case-template.md`: copy into `cases/<UTC-start>-<server>-<short-topic>/case.md`.
- Each case's `artifacts/`: raw captures and derived analyses in distinct paths.

Create case directories with private permissions (`umask 077`). Give capture helpers
new, nonexistent output paths beneath `artifacts/`; do not precreate their output
directories. Keep evidence local. Do not relocate old artifacts just to adopt this
layout; a case may link to their existing absolute paths.

After identifying the deployment, read `ordering.md` and use `index.md` to select
relevant cases. Do not reread the entire archive. Prefer matching symptoms,
workload, tool availability, and versions; identify mismatches before transferring
a lesson. Missing history means use the ordinary skill workflow. Preserve urgent
or transient evidence before spending time on historical review.

## Choose and record the next step

Choose among applicable approaches by expected uncertainty reduction, elapsed time,
server overhead, and prerequisites. Existing required checks and sampler/watch
limits still apply. A learned preference cannot waive evidence coverage checks or
authorize world changes, restarts, new monitoring, or overlapping captures.

For each meaningful attempt, record what was known beforehand, the question or
competing hypotheses, why this approach was selected, and what result could
distinguish them. Include prior step IDs needed to choose the scope or interpret
the result. Then link the raw evidence and record the outcome and costs. Keep this
brief; one attempt can group related commands into a single diagnostic approach.

Use these outcome meanings:

| Outcome | Meaning | Ordering lesson |
| --- | --- | --- |
| Useful evidence | Supports or rules out a hypothesis, narrows the next step, or establishes a healthy baseline | Credit its contribution, even if a later approach identifies the cause |
| Inconclusive | A usable observation did not distinguish explanations, missed the incident, or had insufficient coverage | Record the limiting condition; do not treat it as evidence against the suspected cause |
| Collection failure | The method did not produce usable evidence | Reconsider this method for the affected deployment; record any verified recovery |

Separate successful collection from usefulness. Where a partial collection still
provided useful evidence, record both that contribution and the collection gap.
Untried alternatives have no observed outcome. Multiple retries during one incident
are not independent successful or failed cases.

Measure elapsed time where possible; record available scan/watch overhead and agent
effort if known. Leave unknown costs unknown. Do not infer overhead from total
command duration or pretend different approaches have equal cost.

## Finish a case and update preferences

Record the strongest supported conclusion, confidence, unresolved alternatives,
and whether the cause was independently confirmed or remains a hypothesis. A
healthy baseline or an honest unresolved case is a valid outcome. Any later
confirmation should be a dated addition with evidence, not a rewrite of what was
known during the investigation.

Add a concise index entry. Update an ordering preference only when the case adds
relevant evidence, preserving supporting and conflicting case links. State the
applicable context, observed usefulness and costs, prerequisites, exceptions, and
what would cause reconsideration. A single case supports a tentative preference;
stronger language requires repeated comparable cases. Report counts as observed
cases, not unbiased success probabilities: earlier choices determine which later
approaches get tried. Scope tool-failure preferences to the affected versions and
revisit them after repairs or upgrades. Keep records descriptive, not executable.

Before promoting Y ahead of X, check whether X supplied Y's coordinates, thread,
time window, or interpretation. Credit X if it enabled Y. If independence is unknown,
record earlier use of Y as a proposal to evaluate on a future suitable case, not
as a demonstrated saving. Prefer fresh evidence over an old ranking. Do not perform
extra live captures merely to improve the ranking.

Stop when the baseline or conclusion is adequately supported, or when further
useful observation is unavailable within the bounded diagnostic task. Record the
smallest distinguishing follow-up when unresolved; learning does not justify
extending the investigation indefinitely.

## Optional offline evaluation

When enough cases exist to compare a proposed ordering, reserve whole incidents
for evaluation. Reveal only evidence available at each decision point and keep
later conclusions hidden. Evaluate supported conclusions, appropriate uncertainty,
useful follow-ups, prerequisite handling, and collection cost. Include healthy,
incomplete, and conflicting cases; do not optimize only for fast culprit naming.

An unrecorded query has an unknown result. Preserve capture times and dependencies:
reordering saved observations cannot establish what an earlier live query would
have returned. ZFS snapshots can provide persisted historical state and inputs to
separately arranged reproductions; they do not establish live residency, ticking,
or transient JVM behavior. This protocol does not initiate restores or test servers.

Offline comparisons can expose reasoning errors and suggest ordering changes.
Claims of collection savings require temporally supported evidence or subsequent
comparable investigations. Keep the current procedure as a baseline and record
regressions as well as improvements in `ordering.md`.
