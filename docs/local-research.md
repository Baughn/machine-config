# CachyOS System-Wide Settings (vs. Linux Defaults)

Captured from a live CachyOS system (hostname: saya) on 2026-03-06.

## Kernel

- **Custom CachyOS kernel** (`linux-cachyos 6.19.5-3`) — EEVDF scheduler + LTO (Clang Thin LTO) + AutoFDO + Propeller optimizations
- Also has `linux-cachyos-lts` (BORE scheduler variant) installed
- **HZ=1000** (default is 250 on most distros)
- **NO_HZ_FULL** (full tickless, default is NO_HZ_IDLE)
- **PREEMPT=y** (full preemption, default is PREEMPT_VOLUNTARY on most distros)
- **Transparent Hugepages = always** (many distros default to `madvise`)
- **Packages compiled for znver4** (Zen 4 CPU microarch-optimized repos)

## Sysctl Tunables (`cachyos-settings` + udev rules)

| Setting | CachyOS Value | Linux Default |
|---|---|---|
| `vm.swappiness` | **150** (set by zram udev rule) | 60 |
| `vm.vfs_cache_pressure` | **50** | 100 |
| `vm.dirty_bytes` | **256 MB** | 0 (uses ratio) |
| `vm.dirty_background_bytes` | **64 MB** | 0 (uses ratio) |
| `vm.dirty_writeback_centisecs` | **1500** | 500 |
| `vm.page-cluster` | **0** (optimized for zram) | 3 |
| `kernel.nmi_watchdog` | **0** (disabled) | 1 |
| `kernel.printk` | **3 3 3 3** (quiet) | 4 4 1 7 |
| `kernel.kptr_restrict` | **2** | 0 |
| `kernel.unprivileged_userns_clone` | **1** | varies |
| `kernel.sysrq` | **128** (reboot only) | 16 or 1 |
| `kernel.split_lock_mitigate` | **0** (gaming) | 1 |
| `net.core.netdev_max_backlog` | **4096** | 1000 |
| `fs.file-max` | **2097152** | ~varies |

## Memory / Swap

- **ZRAM** at full RAM size (93 GB), zstd compressed, swap priority 100
- **Zswap disabled** (in favor of ZRAM)
- **THP defrag = defer+madvise** (default: madvise)
- **THP shrinker**: `khugepaged/max_ptes_none = 409` (default 511) — splits THPs with >80% zero-filled pages

## I/O Scheduling (udev rules)

- NVMe: **none** (no scheduler, direct submission)
- SSD (SATA): **mq-deadline**
- HDD: **bfq**
- SATA link power management: **max_performance**
- HDD: `hdparm -B 254 -S 0` (APM near-max perf, no spindown)

## Systemd

- `DefaultTimeoutStartSec` = **15s** (default 90s)
- `DefaultTimeoutStopSec` = **10s** (default 90s)
- `DefaultLimitNOFILE` = **2048:2097152** system / **1024:1048576** user (defaults are much lower)
- Journal max size = **50 MB** (default 10% of filesystem)
- User services get full **cgroup delegation** (cpu, cpuset, io, memory, pids)
- NTP via **Cloudflare** primary, Google + Arch pool fallback

## NVIDIA

- `NVreg_UsePageAttributeTable=1` (better memory management)
- `NVreg_InitializeSystemMemoryAllocations=0` (skip clearing, perf gain)
- `NVreg_DynamicPowerManagement=0x02` (runtime PM)
- `NVreg_EnableS0ixPowerManagement=1`
- `NVreg_RegistryDwords=RmEnableAggressiveVblank=1` (low-latency display)
- Udev runtime PM auto on bind, on on unbind

## Modprobe

- **Watchdog modules blacklisted**: `iTCO_wdt`, `sp5100_tco`
- **ntsync** module auto-loaded (NT synchronization primitives for Wine/Proton)
- AMD GPU: force `amdgpu` over `radeon` for older GCN chips

## Audio / Realtime

- `@audio` group gets **rtprio 99**
- `/dev/rtc0`, `/dev/hpet`, `/dev/cpu_dma_latency` accessible to `audio` group
- HDA Intel power save disabled when on AC power

## Boot

- **Plymouth** splash in initramfs
- Boot cmdline: `quiet nowatchdog splash rw`
- Bootloader: **Limine** (with Snapper snapshot integration)
- Initramfs hooks: `base systemd autodetect microcode kms modconf block keyboard sd-vconsole plymouth filesystems`

## Filesystem

- **Btrfs** with subvolumes (`@`, `@home`, `@root`, `@srv`, `@cache`, `@tmp`, `@log`)
- Mount options: `noatime`, `compress=zstd:1`
- **Snapper** for btrfs snapshots (50 snapshots retained, with limine-snapper-sync for bootable snapshots)
- **tmpfs** on `/tmp`
- **NFS4** mount to `10.171.0.1:/home/svein` at `/tsugumi`
- **fstrim.timer** enabled (periodic TRIM)

## Pacman

- **3 CachyOS znver4-optimized repos** (highest priority, before standard Arch repos)
- `ParallelDownloads = 10`
- `DownloadUser = alpm` (sandboxed downloads)

## Process Scheduling

- **ananicy-cpp** enabled with CachyOS rules (auto-adjusts nice, ionice, scheduling class, OOM score, cgroups per-process)
- `rtkit-daemon` log level capped at `info`

## Security / Firewall

- **UFW** enabled
- Kernel pointer restriction (`kptr_restrict=2`)

## Locale / Region

- `LANG=en_GB.UTF-8`, other LC categories `en_IE.UTF-8`
- Timezone: `Europe/Dublin`
- Hostname: `saya`

## Hardware

- **CPU**: AMD Ryzen 9 7950X3D (16C/32T)
- **GPU**: NVIDIA GeForce RTX 4090 (driver 590.48.01)
- **RAM**: 94 GB
- **Storage**: 3x NVMe SSDs (Btrfs)
