# What makes CachyOS fast, and how to bring it to NixOS

**CachyOS derives its speed from a layered stack of optimizations**: a heavily patched custom kernel with the BORE scheduler, AutoFDO and Propeller profile-guided compilation, full-repo package rebuilds targeting x86-64-v3 with `-O3`, and dozens of system-level sysctl/udev/scheduler tweaks. On a 96-core AMD EPYC server, Phoronix measured CachyOS **11.6% faster than Ubuntu 24.04 LTS** and **~5% faster than vanilla Arch Linux** across 104 benchmarks. The good news for your NixOS migration: much of this is replicable. The kernel optimizations port well via `xddxdd/nix-cachyos-kernel`, system tweaks can be declared in NixOS config, but the full package-level `-march=x86-64-v3` rebuild requires custom binary caches and is the hardest piece to reproduce.

---

## The CachyOS kernel: patches, schedulers, and multi-pass compilation

The `linux-cachyos` kernel ships a **base patchset** applied on top of upstream Linux that includes over a dozen patch categories. The headline items: **BORE scheduler** (Burst-Oriented Response Enhancer) as the default CPU scheduler, **sched-ext** framework for dynamically loadable BPF schedulers, **BBRv3** TCP congestion control replacing BBRv1/Cubic, **NTSync** (Windows NT synchronization primitives for Wine/Proton gaming), **ADIOS** (Adaptive Deadline I/O Scheduler), updated in-kernel **zstd** for faster BTRFS/ZRAM compression, AES-crypto patches exploiting AVX2/AVX-512 instruction sets, AMD P-State and Intel P-State driver improvements, Transparent Huge Pages shrinker enhancements, and KSM (Kernel Samepage Merging) improvements. Utility patches include ACS Override for VFIO passthrough, v4l2loopback, OpenRGB support, and HDR display enablement.

CachyOS provides **ten kernel variants**, each built atop the base patchset. The default `linux-cachyos` uses GCC with BORE + sched-ext. The high-performance `linux-cachyos-lto` uses **Clang + ThinLTO + AutoFDO + Propeller** — a sophisticated multi-pass optimization pipeline where the kernel is compiled, profiled under real workloads via `perf`, then recompiled with the resulting profiles baked in, then profiled again for Propeller binary layout optimization. This process runs three full compilations per architecture target. Other variants include `-bmq` (BMQ scheduler from Project C), `-server` (300Hz, no preemption, stock EEVDF for throughput), `-rt` (real-time preemption), `-hardened`, and `-deckify` (Steam Deck optimized with RCU Lazy).

Key kernel config differences versus vanilla Arch Linux:

| Setting | CachyOS | Vanilla Arch |
|---------|---------|-------------|
| Timer frequency | **1000Hz** | 300Hz |
| Preemption model | **PREEMPT (Full)** | PREEMPT_VOLUNTARY |
| CPU scheduler | **BORE on EEVDF** + sched-ext | Stock EEVDF |
| TCP congestion | **BBRv3** + CAKE qdisc | Cubic |
| Compiler (LTO variant) | **Clang + ThinLTO + AutoFDO + Propeller** | GCC |
| Architecture targets | **x86-64-v3, v4, znver4** builds | Generic x86-64 |
| NTSync | Enabled | Not present |
| NVIDIA modules | Precompiled | DKMS-built |

The **AutoFDO + Propeller** pipeline is particularly notable — CachyOS claims **~10% throughput improvement and ~3% latency reduction** from this kernel-level profiling. Profiles are gathered on both Intel and AMD hardware, merged via LLVM 19's multi-profile support. This was shipped by default starting December 2024 (AutoFDO) and February 2025 (Propeller).

---

## BORE and sched-ext: the scheduler story

The **BORE scheduler**, created by developer firelzrd, is CachyOS's default CPU scheduler and a key driver of its desktop responsiveness. BORE enhances EEVDF by tracking each task's "burstiness" — the cumulative CPU time consumed since the task last yielded, slept, or waited for I/O. It computes a burst score (0–39 range) using a binary-to-common logarithm conversion, where **less greedy tasks (interactive applications that frequently yield) receive lower scores and thus get longer timeslices and more aggressive wakeup preemption priority**. CPU-bound tasks accumulate higher burst scores and get deprioritized. Historical scores are smoothed via exponential moving average. The practical effect: dragging windows during a 16-thread kernel compilation feels as smooth as an idle system, according to independent reviewers.

The **sched-ext** framework (upstream since kernel 6.12, adopted by CachyOS in late 2023) enables loading BPF-based CPU schedulers at runtime without rebooting. CachyOS manages these via `scx_loader` (D-Bus daemon) or a GUI in the Kernel Manager. The most important available schedulers:

- **scx_bpfland** — The recommended default. vruntime-based with interactive workload priority, L2/L3 cache-aware topology, per-CPU dispatch queues, and profiles for gaming/lowlatency/powersave/server. Developed by Andrea Righi.
- **scx_lavd** (Latency-criticality Aware Virtual Deadline) — Developed by Changwoo Min at Igalia specifically for the Steam Deck. Features Core Compaction (idle cores stay in C-state when CPU utilization is below 50%) and autopilot power modes. Benchmarked at **5.2% higher average FPS** in Baldur's Gate 3 on Steam Deck.
- **scx_rusty** — NUMA-aware load balancer partitioning CPUs into scheduling domains (one per LLC) with greedy cross-NUMA stealing. Best for multi-socket servers.
- **scx_flash**, **scx_layered**, **scx_p2dq**, **scx_nest** — Specialized schedulers for fairness, cgroup-based policies, LLC balancing, and cache locality respectively.

---

## System-level tweaks: the cachyos-settings stack

The `CachyOS-Settings` package (GitHub: `CachyOS/CachyOS-Settings`) is where CachyOS's non-kernel performance tuning lives. It contains sysctl configs, udev rules, modprobe settings, systemd overrides, and tmpfiles.d configurations.

**Memory management** is aggressively tuned for ZRAM. CachyOS uses `zram-generator` with **zstd compression**, sized equal to physical RAM, as the sole swap mechanism (no swap partition). When ZRAM is detected, a udev rule sets `vm.swappiness` to **150** (far above the typical 60 default) because decompressing from ZRAM in RAM is orders of magnitude cheaper than disk I/O. `vm.page-cluster` is set to **0** (disabled) since compressed ZRAM pages aren't sequential. Zswap is explicitly disabled to prevent it from intercepting pages before ZRAM. Dirty bytes are set as absolute values rather than ratios for predictable I/O behavior, and `kernel.nmi_watchdog` is disabled to reduce CPU overhead.

**I/O scheduler assignment** via udev rules follows a device-type hierarchy: **bfq** for rotational HDDs, **mq-deadline** for SATA SSDs, and **none** (hardware passthrough) for NVMe drives. The kernel also includes ADIOS, an adaptive deadline I/O scheduler that predicts completion latency from historical data and uses a 4-tier priority system.

**Process priority management** uses **ananicy-cpp**, a C++ rewrite of the ANother Auto NICe daemon. It runs as a systemd service and automatically adjusts nice levels, I/O class, I/O priority, scheduling policy, latency_nice, OOM score, and cgroup placement for known processes. CachyOS ships extensive rules categorizing processes: games get negative nice and best-effort I/O priority, background CPU/IO tasks get nice 16 with idle I/O class, chat applications get nice -3, and heavy CPU tasks get nice 9 with best-effort I/O class 7. The `latency_nice` kernel hint is supported for scheduler-aware prioritization.

**Network tuning** is substantial: BBRv3 congestion control with CAKE queue discipline, TCP Fast Open enabled for both client and server (mode 3), dramatically increased buffer sizes (`rmem_max` and `wmem_max` at **16MB**, `netdev_max_backlog` at **16384**, `somaxconn` at **8192**), aggressive keepalive settings (60s time, 10s interval), and MTU probing enabled.

**Filesystem defaults** use BTRFS with `compress=zstd` (level 3), `space_cache=v2`, and `commit=120` (4× the default, prioritizing desktop responsiveness over data-loss window). Subvolume layout splits `@`, `@home`, `@var`, `@tmp`, `@srv` out of the box.

Additional system tweaks include: Transparent Huge Pages defrag set to `defer+madvise`, KSM enabled with 500ms scan interval, systemd service timeouts shortened to 15s start / 10s stop (versus default 90s), journal size capped at 50MB, file descriptor limits raised to 2M for system services, NVIDIA driver optimizations (PAT enabled, memory clearing disabled, dynamic power management, aggressive VBlank), and audio latency tuning (PCI latency set to 80 cycles for sound cards, `snd-hda-intel` power saving disabled).

---

## Package-level optimizations: the full-repo x86-64-v3 rebuild

CachyOS maintains **~10 separate package repositories** covering three architecture tiers beyond generic x86-64. The `makepkg.conf` uses these key flags:

- **`-march=x86-64-v3`** (or `-march=x86-64-v4` / `-march=znver4` for respective repos)
- **`-O3`** (versus Arch's default `-O2`)
- **`-mpclmul`** (carry-less multiplication)
- **`-falign-functions=32`** (function alignment)
- **`-flto`** (Link-Time Optimization enabled by default)
- Rust equivalents: `-Ctarget-cpu=x86-64-v3`, `-Copt-level=3`
- Go equivalent: `GOAMD64=v3`

**All packages in Arch's core and extra repos are rebuilt** with these flags for each architecture level. Architecture-independent packages (`any` arch) are excluded. The optimized repos (`cachyos-core-v3`, `cachyos-extra-v3`, etc.) are placed above standard Arch repos in `pacman.conf` so they take priority. An independent analysis by sunnyflunk found that **`-O3` contributes significantly to gains** — sometimes more than the march level itself — though results are workload-dependent, with some packages (bzip2, xz) showing regressions.

Beyond recompilation, CachyOS maintains **custom PKGBUILDs** (in `CachyOS/CachyOS-PKGBUILDS`, 258+ stars) for packages receiving additional patches: mesa-git (bleeding-edge Mesa with AMD Anti-Lag 2 and FSR4), mutter-cachyos (patched GNOME compositor), proton-cachyos (Wine-Wayland patches, Anti-Lag 2 for vkd3d-proton, DualSense haptics), NVIDIA packages with extensive kernel compatibility patches, and formerly cachy-browser (deprecated May 2025, replaced by `firefox-pure` with CachyOS settings overlay).

The build infrastructure uses a **forked Arch devtools** toolchain and a custom Rust-based `repo-manage-util` with PostgreSQL tracking, Docker containerized builds, and CDN77-backed mirror distribution. CachyOS ships a forked `pacman` with `INSTALLED_FROM` tracking and automatic CPU architecture detection.

---

## Benchmarks confirm measurable, consistent gains

Phoronix has benchmarked CachyOS extensively since 2022, with coverage intensifying as optimizations accumulated. The most compelling results from Michael Larabel's December 2025 test on a **96-core AMD EPYC 9655P**: CachyOS placed **first in 55% of 104 benchmarks**, running **11.6% faster than Ubuntu 24.04 LTS**, **9.1% faster than Ubuntu 25.10**, and **~5% faster than vanilla Arch Linux** on geometric mean — with no meaningful difference in CPU power consumption (~243-248W across all distros tested).

On desktop hardware, CachyOS surpassed Intel's Clear Linux on Arrow Lake by ~4% over Ubuntu (a notable milestone, as Clear Linux was long considered the performance king). After Intel discontinued Clear Linux in 2025, CachyOS is widely seen as its spiritual successor. In a November 2025 Framework Desktop test (AMD Ryzen AI Max+ 395), CachyOS delivered "notably better performance overall" versus both Ubuntu 25.10 and Fedora 43, with openSUSE Tumbleweed earning second place.

Gaming benchmarks show more nuanced results. CachyOS's developer ptr1337 acknowledged that **optimizations primarily impact CPU-bound scenarios** — Steam games run inside the Steam Linux Runtime (a Debian-based container), which limits distro package optimization impact for Proton titles. Native Linux games benefit more directly. A February 2026 benchmark (CachyOS vs Bazzite vs Windows 11 on RX 9070 XT) showed CachyOS matching or slightly exceeding Bazzite in most titles, with a standout **23% improvement in 1%-low frametimes** in Where Winds Meet.

Community consensus on what matters most, ranked by impact: **(1) x86-64-v3 package repos** (5-20% for compute-heavy workloads), **(2) newer kernel + patches** (especially for gaming/drivers), **(3) AutoFDO + Propeller kernel optimization** (~10% throughput), **(4) BORE/sched-ext** (major for perceived responsiveness under load, less for raw throughput), **(5) system tuning** (sysctl, I/O schedulers, ananicy-cpp).

---

## Porting to NixOS: what's feasible and what's hard

The **kernel** is the easiest piece to replicate. The primary option is **`xddxdd/nix-cachyos-kernel`** (the successor to the now-archived Chaotic Nyx flake). It supports multiple kernel variants, configurable x86-64-v1 through v4 and znver4 march targeting, AutoFDO, Propeller, ThinLTO with Clang, timer frequency, preemption type, and ZFS support. A binary cache via Hydra CI is available. A NixOS Discourse user reported **~10% single-core and ~8% multi-core Geekbench improvement** simply by swapping to the CachyOS LTO kernel on NixOS. There is also an active discussion (nixpkgs issue #214819, PR #330917) to include CachyOS kernels as first-class packages in nixpkgs.

**System-level tweaks** translate naturally to NixOS's declarative configuration. You can set all the sysctl values (`boot.kernel.sysctl`), configure ZRAM via `zramSwap`, set I/O schedulers via udev rules (`services.udev.extraRules`), configure ananicy-cpp (available in nixpkgs), set filesystem mount options, configure systemd service parameters, and apply network tuning — all in `configuration.nix`. These are arguably easier to manage on NixOS than on Arch.

The **full package-level x86-64-v3 rebuild** is the hardest to replicate. NixOS would need a custom Hydra/binary cache rebuilding the entire package set with `-march=x86-64-v3 -O3`. One NixOS user described setting up a personal build server for exactly this purpose. The `nixpkgs` `stdenv` can be overridden with custom `CFLAGS`, but maintaining a full parallel binary cache is a significant infrastructure commitment. A pragmatic approach: selectively rebuild performance-critical packages (mesa, glibc, the kernel, media codecs) while accepting generic builds for the long tail.

Here is a practical NixOS configuration checklist derived from CachyOS's optimizations:

- **Kernel**: Use `xddxdd/nix-cachyos-kernel` with BORE + sched-ext, ThinLTO, x86-64-v3, 1000Hz timer, full preemption
- **ZRAM**: Enable `zramSwap` with zstd, size = RAM, swappiness = 150, page-cluster = 0
- **Sysctl**: Disable nmi_watchdog, set BBR congestion control, CAKE qdisc, increase network buffers, set dirty_bytes
- **I/O**: udev rules for bfq (HDD), mq-deadline (SATA SSD), none (NVMe)
- **ananicy-cpp**: Enable the service, import CachyOS ananicy-rules
- **Filesystem**: BTRFS with compress=zstd, commit=120
- **THP**: defrag = defer+madvise, shmem_enabled = advise
- **Systemd**: Shorten timeouts, increase FD limits, cap journal size
- **sched-ext**: Install scx-scheds, use scx_bpfland for desktop or scx_lavd for gaming

## Conclusion

CachyOS's performance advantage is not one single trick but a **compound effect of 50+ individual optimizations** spanning every layer of the stack. The largest measurable gains come from x86-64-v3 package rebuilds and the AutoFDO/Propeller-optimized kernel, while the BORE scheduler and system tuning drive the subjective desktop responsiveness that users praise. For a NixOS migration, the kernel (~60% of the benefit) and system tweaks (~20%) are straightforward to replicate declaratively. The full package rebuild (~20%) requires serious infrastructure but can be approximated by targeting critical packages. The CachyOS GitHub organization (`github.com/CachyOS`, 112 repos) is exhaustively documented — `linux-cachyos`, `kernel-patches`, `CachyOS-Settings`, `CachyOS-PKGBUILDS`, and `ananicy-rules` are the five repos to study closely.
