# KWin Wayland Atomic Modeset Failure on NVIDIA

## Summary

KWin's DRM backend fails to initialize displays on NVIDIA GPUs when multiple
monitors are connected at high refresh rates. The atomic modeset test returns
`EINVAL` for every CRTC-connector permutation, resulting in a black screen with
cursor.

Gnome/Mutter works on the same hardware because it starts all monitors at 60Hz,
well within bandwidth limits.

## Hardware

- GPU: NVIDIA GeForce RTX 4090 (AD102-A)
- Driver: 595.45.04, open kernel module
- Kernel: CachyOS 6.19.9 (zen4)
- Monitors (4 connected):
  - Dell AW2725DF (DFP-0): 2560x1440, max 360Hz
  - Asus ROG PG279Q (DFP-1): 2560x1440, max 144Hz
  - Dell AW3423DW (DFP-3): 3440x1440, max 175Hz
  - Dell AW2725DF (DFP-5): 2560x1440, max 360Hz

## Symptoms

- SDDM with `sddm.wayland.compositor = "kwin"` shows black screen on all monitors
- Terminal cursor is visible on one monitor
- `journalctl` shows: `kwin_wayland_drm: Atomic modeset test failed! Invalid argument`
- Happens identically in both greeter and full Plasma session

## Root Cause

KWin requests all monitors at their maximum refresh rate in the initial atomic
modeset commit. The NVIDIA driver rejects this because the combined pixel clock
exceeds the GPU's display bandwidth:

| Monitor       | KWin requests      | Gnome requests     |
|---------------|--------------------|--------------------|
| AW2725DF      | 2560x1440@143.97   | 2560x1440@59.95    |
| PG279Q        | 2560x1440@144.00   | 2560x1440@59.95    |
| AW3423DW      | 3440x1440@174.96   | 3440x1440@59.97    |
| AW2725DF      | 2560x1440@359.98   | 2560x1440@59.95    |
| **Total clock** | **~3.85 GHz**    | **~1.04 GHz**      |

KWin then tries all 24 permutations of CRTC-to-connector assignment (4! = 24),
all with the same max refresh rates, and all fail. It never tries lower refresh
rates.

### Additional property differences

KWin also sets properties that Gnome omits in the initial commit:
- `DEGAMMA_LUT = 0` (Gnome omits this entirely)
- `VRR_ENABLED = 0` (Gnome omits this)
- `link-status = 0` on connectors (Gnome omits this)

These may independently contribute to the failure, but the bandwidth is the
most likely primary cause.

## Methodology

We built an `LD_PRELOAD` shim (`drm-atomic-log`) that intercepts `ioctl()`
calls, filters for `DRM_IOCTL_MODE_ATOMIC`, and serializes the full commit
(object IDs, property names/values, mode blobs, flags, return codes) to
human-readable log files.

Logs were captured for both KWin (via SDDM) and Mutter (via GDM) on the same
hardware and compared.

### KWin log summary

- 24 atomic commits, all `TEST_ONLY | ALLOW_MODESET`
- All 24 return `EINVAL`
- Each commit tries all 4 monitors at max refresh
- The 24 commits are all permutations of CRTC-to-connector assignment
- KWin never attempts lower refresh rates or fewer monitors

### Gnome log summary

- 129 atomic commits
- 127 succeed, 2 fail with `EBUSY` (normal race)
- First commit: all 4 monitors at 60Hz, succeeds
- Flags: `PAGE_FLIP_EVENT | ALLOW_MODESET` (not `TEST_ONLY`)

## Raw Logs

- `./log-sddm/` — KWin/SDDM capture (drm-atomic-16081.log)
- `./log-gdm/` — Gnome/GDM capture (drm-atomic-5465.log)

## Logger Source

- `drm-atomic-log.c` — LD_PRELOAD shim (in this directory)
- `drm-atomic-log.nix` — NixOS module to inject the shim into display managers
