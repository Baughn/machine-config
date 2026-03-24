# Future Plans

## Confirmed: Root Cause

Forcing `chooseMode()` to pick ~60Hz (by flipping the refresh rate comparator)
makes all 4 monitors work. 229 atomic commits, zero failures.

Two bugs identified:

### Bug 1: No refresh rate fallback on atomic test failure

`chooseMode()` picks the highest refresh rate at native resolution. When the
combined bandwidth across all monitors exceeds NVIDIA's limits, the atomic
modeset test fails with EINVAL. The existing fallback
(`m_forceLowBandwidthMode`) only reduces color depth, not refresh rate.

KWin then tries all 4! = 24 CRTC-connector permutations, all at the same
max refresh rates, and all fail. It never tries lower refresh rates.

### Bug 2: Config saved before validation

`storeConfig()` in `queryConfig()` persists the output config to
`kwinoutputconfig.json` *before* the atomic test runs. On subsequent
startups, `findSetup()` finds this saved config and uses it directly,
bypassing `chooseMode()` entirely. This means even if the modes were
never successfully applied, they get reloaded forever.

For the SDDM greeter, this file is at:
`/var/lib/sddm/.config/kwinoutputconfig.json`

## Next: Proper KWin patch

### Fix for Bug 1: Refresh rate fallback in `testPendingConfiguration()`

In `drm_gpu.cpp`, after the `m_forceLowBandwidthMode` retry fails, add a
third fallback that regenerates the config with lower refresh rates.

The cleanest approach: add a `bool preferLowRefreshRate` parameter to
`chooseMode()` and `generateConfig()`. When set, pick the lowest refresh
rate >= 50Hz at native resolution instead of the highest.

Flow:
```
testPendingConfiguration():
  1. Try normal                          → EINVAL
  2. Try m_forceLowBandwidthMode=true    → EINVAL
  3. NEW: regenerate config with preferLowRefreshRate=true, retry
```

Files:
- `src/outputconfigurationstore.cpp` — add `preferLowRefreshRate` param
- `src/outputconfigurationstore.h` — update signatures
- `src/backends/drm/drm_gpu.cpp` — add third fallback

### Fix for Bug 2: Don't persist unvalidated configs

`storeConfig()` should only be called after the atomic test succeeds,
not unconditionally in `queryConfig()`. Or: mark the stored config as
"unvalidated" and re-test it on load.

## Existing KDE bugs

- [Bug 509635](https://bugs.kde.org/show_bug.cgi?id=509635) — closest match,
  bandwidth-related black screen, partially fixed but doesn't cover refresh rate
- [Bug 513601](https://bugs.kde.org/show_bug.cgi?id=513601) — KWin chooses
  highest refresh rate by default, users want 60Hz fallback
- [Bug 455532](https://bugs.kde.org/show_bug.cgi?id=455532) — "Failed to find
  a working setup for new outputs"
