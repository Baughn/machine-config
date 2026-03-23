# Future Plans

## Immediate: Find or file KWin bug

- [ ] Search KDE Bugzilla / KDE Invent for existing reports of this issue
  - Keywords: "atomic modeset", "NVIDIA", "multi-monitor", "refresh rate",
    "bandwidth", "EINVAL"
- [ ] Check KWin's DRM backend source (`src/backends/drm/`) for how it selects
  initial modes — does it always pick preferred/max modes?
- [ ] If no existing bug, file one with the data from this investigation
  - Include: hardware details, log comparison, root cause analysis

## Short-term: KWin patch

The fix should be in KWin's DRM backend. When the initial atomic modeset test
fails with `EINVAL`, KWin should:

1. Fall back to lower refresh rates (e.g. 60Hz) for all monitors
2. If that succeeds, ramp up refresh rates one monitor at a time
3. Or: start at 60Hz and let the user configure higher rates via display settings

This matches Gnome/Mutter's behavior, which starts at 60Hz and works.

Relevant KWin source locations to investigate:
- `src/backends/drm/drm_gpu.cpp` — GPU/output initialization
- `src/backends/drm/drm_pipeline.cpp` — atomic commit construction
- `src/backends/drm/drm_output.cpp` — mode selection

## Medium-term: Determine if NVIDIA or KWin is at fault

The NVIDIA driver returning `EINVAL` for a configuration that exceeds bandwidth
is arguably correct behavior. The question is whether:

1. **KWin should handle this gracefully** — try lower refresh rates when atomic
   test fails (this is the practical fix)
2. **NVIDIA should provide better error reporting** — the driver could return a
   more specific error or expose bandwidth limits via DRM properties
3. **Both** — KWin should be resilient, and NVIDIA should be informative

## Other observations

- KWin sets `DEGAMMA_LUT`, `VRR_ENABLED`, and `link-status` on connectors in
  its initial commit. Gnome omits these. These properties may independently
  cause EINVAL on this driver version, worth testing in isolation.
- The shim could be extended to *modify* atomic commits (e.g. force 60Hz modes)
  as a workaround, but a proper KWin fix is preferable.
