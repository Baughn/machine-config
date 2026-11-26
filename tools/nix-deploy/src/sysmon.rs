//! Linux system telemetry sampler for CPU, memory (with ZFS ARC accounting),
//! and 1-minute load average.
//!
//! Reads directly from `/proc` using only the Rust standard library and keeps
//! bounded ring buffers suitable for rendering live sparklines.

use std::collections::VecDeque;
use std::fs;

/// Maximum number of historical samples retained in CPU and memory ring buffers.
pub const HISTORY_LEN: usize = 120;

const PROC_STAT_PATH: &str = "/proc/stat";
const PROC_MEMINFO_PATH: &str = "/proc/meminfo";
const ZFS_ARCSTATS_PATH: &str = "/proc/spl/kstat/zfs/arcstats";
const PROC_LOADAVG_PATH: &str = "/proc/loadavg";

const KIB: u64 = 1024;
const MIB_F64: f64 = 1024.0 * 1024.0;
const GIB: u64 = 1024 * 1024 * 1024;

/// Raw aggregate CPU tick counters from the `cpu ` line of `/proc/stat`.
///
/// The Linux kernel already includes `guest` in `user` and `guest_nice` in
/// `nice`, so these eight fields form the exact total tick count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    /// Time spent in user mode.
    pub user: u64,
    /// Time spent in low-priority (niced) user mode.
    pub nice: u64,
    /// Time spent in kernel mode.
    pub system: u64,
    /// Time spent idle.
    pub idle: u64,
    /// Time spent waiting for I/O to complete.
    pub iowait: u64,
    /// Time servicing hardware interrupts.
    pub irq: u64,
    /// Time servicing software interrupts.
    pub softirq: u64,
    /// Stolen time when running in a virtualized environment.
    pub steal: u64,
}

impl CpuTimes {
    /// Total idle ticks (`idle + iowait`).
    ///
    /// Uses `saturating_add` so integer overflow clamps at `u64::MAX` instead
    /// of panicking in debug builds or wrapping in release builds.
    pub fn idle_total(self) -> u64 {
        self.idle.saturating_add(self.iowait)
    }

    /// Sum of all eight CPU tick counters.
    pub fn total(self) -> u64 {
        self.user
            .saturating_add(self.nice)
            .saturating_add(self.system)
            .saturating_add(self.idle)
            .saturating_add(self.iowait)
            .saturating_add(self.irq)
            .saturating_add(self.softirq)
            .saturating_add(self.steal)
    }
}

/// Parsed memory totals from `/proc/meminfo`, converted from KiB to bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    /// Total physical memory in bytes (`MemTotal`).
    pub mem_total_bytes: u64,
    /// Available memory in bytes (`MemAvailable`, or fallback `MemFree + Buffers + Cached`).
    pub mem_available_bytes: u64,
}

/// Parsed ZFS Adaptive Replacement Cache (ARC) sizing from `/proc/spl/kstat/zfs/arcstats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArcStats {
    /// Current total ARC size in bytes (`size`).
    pub size_bytes: u64,
    /// Minimum non-reclaimable ARC size in bytes (`c_min`).
    pub c_min_bytes: u64,
}

impl ArcStats {
    /// Reclaimable portion of the ZFS ARC (`size.saturating_sub(c_min)`), matching `btop`/`htop`.
    pub fn reclaimable_bytes(self) -> u64 {
        self.size_bytes.saturating_sub(self.c_min_bytes)
    }
}

/// Computed memory usage after subtracting reclaimable ZFS ARC from raw used RAM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemUsage {
    /// Non-reclaimable memory in use, in bytes.
    pub mem_used_bytes: u64,
    /// Total physical memory, in bytes.
    pub mem_total_bytes: u64,
    /// Used memory percentage in `0.0..=100.0`.
    pub mem_pct: f64,
    /// Total ZFS ARC size in bytes, or `None` when ZFS ARC stats are unavailable.
    pub arc_bytes: Option<u64>,
    /// Reclaimable ZFS ARC bytes subtracted from raw used memory (`0` if unavailable).
    pub reclaimable_arc_bytes: u64,
}

/// A single point-in-time system telemetry snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Aggregate CPU utilization percentage in `0.0..=100.0`.
    pub cpu_pct: f64,
    /// Effective memory utilization percentage in `0.0..=100.0`.
    pub mem_pct: f64,
    /// Effective used memory in bytes (excluding reclaimable ZFS ARC).
    pub mem_used_bytes: u64,
    /// Total physical memory in bytes.
    pub mem_total_bytes: u64,
    /// Current total ZFS ARC size in bytes, if `/proc/spl/kstat/zfs/arcstats` is present.
    pub arc_bytes: Option<u64>,
    /// Reclaimable ZFS ARC bytes (`size.saturating_sub(c_min)`), or `0` if unavailable.
    pub reclaimable_arc_bytes: u64,
    /// 1-minute system load average from `/proc/loadavg`, if available.
    pub load1: Option<f64>,
}

/// Parse the aggregate `cpu ` line from `/proc/stat` contents.
///
/// # Arguments
///
/// * `contents` - Full text read from `/proc/stat`.
///
/// # Returns
///
/// `Some(CpuTimes)` if an aggregate `cpu` line with at least `user`, `nice`,
/// `system`, and `idle` counters is present; `None` otherwise.
pub fn parse_proc_stat(contents: &str) -> Option<CpuTimes> {
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("cpu") {
            continue;
        }

        let user = parts.next()?.parse::<u64>().ok()?;
        let nice = parts.next()?.parse::<u64>().ok()?;
        let system = parts.next()?.parse::<u64>().ok()?;
        let idle = parts.next()?.parse::<u64>().ok()?;

        let iowait = parse_optional_u64(parts.next())?;
        let irq = parse_optional_u64(parts.next())?;
        let softirq = parse_optional_u64(parts.next())?;
        let steal = parse_optional_u64(parts.next())?;

        return Some(CpuTimes {
            user,
            nice,
            system,
            idle,
            iowait,
            irq,
            softirq,
            steal,
        });
    }

    None
}

/// Parse an optional counter token: `None` means absent (`Some(0)`), while an
/// unparseable token propagates `None` so malformed lines are rejected.
fn parse_optional_u64(token: Option<&str>) -> Option<u64> {
    match token {
        Some(raw) => raw.parse::<u64>().ok(),
        None => Some(0),
    }
}

/// Compute aggregate CPU busy percentage between two consecutive [`CpuTimes`] readings.
///
/// # Arguments
///
/// * `prev` - Earlier CPU tick snapshot.
/// * `curr` - Newer CPU tick snapshot.
///
/// # Returns
///
/// Busy percentage clamped to `0.0..=100.0`. Returns `0.0` when `total_delta == 0`.
pub fn compute_cpu_pct(prev: CpuTimes, curr: CpuTimes) -> f64 {
    let total_delta = curr.total().saturating_sub(prev.total());
    if total_delta == 0 {
        return 0.0;
    }

    let idle_delta = curr.idle_total().saturating_sub(prev.idle_total());
    let busy_delta = total_delta.saturating_sub(idle_delta);
    (busy_delta as f64 / total_delta as f64 * 100.0).clamp(0.0, 100.0)
}

/// Parse `MemTotal` and `MemAvailable` (falling back to `MemFree + Buffers + Cached`)
/// from `/proc/meminfo` contents, returning values in bytes.
///
/// # Arguments
///
/// * `contents` - Full text read from `/proc/meminfo`.
///
/// # Returns
///
/// `Some(MemInfo)` with byte values when `MemTotal` and availability fields are present;
/// `None` otherwise.
pub fn parse_proc_meminfo(contents: &str) -> Option<MemInfo> {
    let mut mem_total_kib: Option<u64> = None;
    let mut mem_available_kib: Option<u64> = None;
    let mut mem_free_kib: Option<u64> = None;
    let mut buffers_kib: Option<u64> = None;
    let mut cached_kib: Option<u64> = None;

    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(val_str) = parts.next() else {
            continue;
        };

        match key {
            "MemTotal:" => mem_total_kib = val_str.parse::<u64>().ok(),
            "MemAvailable:" => mem_available_kib = val_str.parse::<u64>().ok(),
            "MemFree:" => mem_free_kib = val_str.parse::<u64>().ok(),
            "Buffers:" => buffers_kib = val_str.parse::<u64>().ok(),
            "Cached:" => cached_kib = val_str.parse::<u64>().ok(),
            _ => {}
        }
    }

    let total_kib = mem_total_kib?;
    let available_kib = match mem_available_kib {
        Some(avail) => avail,
        None => {
            if mem_free_kib.is_none() && buffers_kib.is_none() && cached_kib.is_none() {
                return None;
            }
            mem_free_kib
                .unwrap_or(0)
                .saturating_add(buffers_kib.unwrap_or(0))
                .saturating_add(cached_kib.unwrap_or(0))
        }
    };

    Some(MemInfo {
        mem_total_bytes: total_kib.saturating_mul(KIB),
        mem_available_bytes: available_kib.saturating_mul(KIB),
    })
}

/// Parse `size` and `c_min` (in bytes) from `/proc/spl/kstat/zfs/arcstats`.
///
/// # Arguments
///
/// * `contents` - Full text read from `/proc/spl/kstat/zfs/arcstats`.
///
/// # Returns
///
/// `Some(ArcStats)` if both `size` and `c_min` rows are present and valid integers;
/// `None` otherwise.
pub fn parse_arcstats(contents: &str) -> Option<ArcStats> {
    let mut size_bytes: Option<u64> = None;
    let mut c_min_bytes: Option<u64> = None;

    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        // kstat rows have three columns: `<name> <type> <value>`.
        let Some(_kstat_type) = parts.next() else {
            continue;
        };
        let Some(val_str) = parts.next() else {
            continue;
        };

        match name {
            "size" => size_bytes = val_str.parse::<u64>().ok(),
            "c_min" => c_min_bytes = val_str.parse::<u64>().ok(),
            _ => {}
        }
    }

    Some(ArcStats {
        size_bytes: size_bytes?,
        c_min_bytes: c_min_bytes?,
    })
}

/// Parse the 1-minute load average from `/proc/loadavg`.
///
/// # Arguments
///
/// * `contents` - Full text read from `/proc/loadavg`.
///
/// # Returns
///
/// `Some(f64)` if the first whitespace-delimited token is a finite, non-negative float;
/// `None` otherwise.
pub fn parse_loadavg(contents: &str) -> Option<f64> {
    let first = contents.split_whitespace().next()?;
    let load1 = first.parse::<f64>().ok()?;
    (load1.is_finite() && load1 >= 0.0).then_some(load1)
}

/// Compute effective memory usage by subtracting reclaimable ZFS ARC from raw used memory.
///
/// # Arguments
///
/// * `mem` - Total and available memory parsed from `/proc/meminfo`.
/// * `arc` - Optional ZFS ARC statistics parsed from `/proc/spl/kstat/zfs/arcstats`.
///
/// # Returns
///
/// [`MemUsage`] with `mem_pct` guaranteed finite and in `0.0..=100.0` (`0.0` if total is `0`).
pub fn compute_mem_usage(mem: MemInfo, arc: Option<ArcStats>) -> MemUsage {
    let reclaimable_arc_bytes = arc.map_or(0, ArcStats::reclaimable_bytes);
    let raw_used = mem.mem_total_bytes.saturating_sub(mem.mem_available_bytes);
    let mem_used_bytes = raw_used.saturating_sub(reclaimable_arc_bytes);

    let mem_pct = if mem.mem_total_bytes == 0 {
        0.0
    } else {
        (mem_used_bytes as f64 / mem.mem_total_bytes as f64 * 100.0).clamp(0.0, 100.0)
    };

    MemUsage {
        mem_used_bytes,
        mem_total_bytes: mem.mem_total_bytes,
        mem_pct,
        arc_bytes: arc.map(|a| a.size_bytes),
        reclaimable_arc_bytes,
    }
}

/// Stateful Linux system monitor maintaining bounded CPU and memory sparkline histories.
#[derive(Debug, Clone)]
pub struct SysMon {
    prev_cpu: Option<CpuTimes>,
    latest: Option<Sample>,
    cpu_history: VecDeque<u64>,
    mem_history: VecDeque<u64>,
}

impl Default for SysMon {
    fn default() -> Self {
        Self::new()
    }
}

impl SysMon {
    /// Create a new monitor and capture an initial `/proc/stat` baseline so the first
    /// call to [`SysMon::sample`] yields a real CPU utilization delta.
    pub fn new() -> Self {
        let initial_cpu = read_proc_stat();
        Self::with_baseline(initial_cpu)
    }

    fn with_baseline(prev_cpu: Option<CpuTimes>) -> Self {
        Self {
            prev_cpu,
            latest: None,
            cpu_history: VecDeque::with_capacity(HISTORY_LEN),
            mem_history: VecDeque::with_capacity(HISTORY_LEN),
        }
    }

    /// Sample `/proc` telemetry, update the latest snapshot, and append rounded `0..=100`
    /// percentages to the CPU and memory history ring buffers (capped at [`HISTORY_LEN`]).
    pub fn sample(&mut self) -> Sample {
        let cpu = read_proc_stat();
        let mem = read_proc_meminfo();
        let arc = read_arcstats();
        let load1 = read_loadavg();
        self.record_sample(cpu, mem, arc, load1)
    }

    fn record_sample(
        &mut self,
        cpu: Option<CpuTimes>,
        mem: Option<MemInfo>,
        arc: Option<ArcStats>,
        load1: Option<f64>,
    ) -> Sample {
        let cpu_pct = match (self.prev_cpu, cpu) {
            (Some(prev), Some(curr)) => {
                self.prev_cpu = Some(curr);
                compute_cpu_pct(prev, curr)
            }
            (None, Some(curr)) => {
                self.prev_cpu = Some(curr);
                0.0
            }
            (_, None) => 0.0,
        };

        let mem_usage = match mem {
            Some(info) => compute_mem_usage(info, arc),
            None => MemUsage {
                mem_used_bytes: 0,
                mem_total_bytes: 0,
                mem_pct: 0.0,
                arc_bytes: arc.map(|a| a.size_bytes),
                reclaimable_arc_bytes: arc.map_or(0, ArcStats::reclaimable_bytes),
            },
        };

        let sample = Sample {
            cpu_pct,
            mem_pct: mem_usage.mem_pct,
            mem_used_bytes: mem_usage.mem_used_bytes,
            mem_total_bytes: mem_usage.mem_total_bytes,
            arc_bytes: mem_usage.arc_bytes,
            reclaimable_arc_bytes: mem_usage.reclaimable_arc_bytes,
            load1,
        };

        push_capped(&mut self.cpu_history, round_pct(sample.cpu_pct));
        push_capped(&mut self.mem_history, round_pct(sample.mem_pct));
        self.latest = Some(sample.clone());
        sample
    }

    /// Return the most recently recorded [`Sample`], or `None` before the first [`Self::sample`].
    pub fn latest(&self) -> Option<&Sample> {
        self.latest.as_ref()
    }

    /// Return the bounded ring buffer of rounded CPU percentages (`0..=100`).
    pub fn cpu_history(&self) -> &VecDeque<u64> {
        &self.cpu_history
    }

    /// Return the bounded ring buffer of rounded memory percentages (`0..=100`).
    pub fn mem_history(&self) -> &VecDeque<u64> {
        &self.mem_history
    }

    /// Format a byte count for compact UI display: `"18.4G"` for 1 GiB and above,
    /// or `"512M"` for values below 1 GiB.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Size in bytes.
    ///
    /// # Returns
    ///
    /// Human-readable binary-prefixed size string (`M` or `G`).
    pub fn format_gib(bytes: u64) -> String {
        if bytes >= GIB {
            format!("{:.1}G", bytes as f64 / GIB as f64)
        } else {
            let mib = (bytes as f64 / MIB_F64).round() as u64;
            if mib >= 1024 {
                "1.0G".to_string()
            } else {
                format!("{mib}M")
            }
        }
    }
}

fn push_capped(buf: &mut VecDeque<u64>, value: u64) {
    // VecDeque supports O(1) pop_front and push_back, making it an efficient ring buffer.
    if buf.len() >= HISTORY_LEN {
        buf.pop_front();
    }
    buf.push_back(value);
}

fn round_pct(pct: f64) -> u64 {
    if !pct.is_finite() || pct <= 0.0 {
        0
    } else {
        (pct.round() as u64).min(100)
    }
}

fn read_proc_stat() -> Option<CpuTimes> {
    let contents = fs::read_to_string(PROC_STAT_PATH).ok()?;
    parse_proc_stat(&contents)
}

fn read_proc_meminfo() -> Option<MemInfo> {
    let contents = fs::read_to_string(PROC_MEMINFO_PATH).ok()?;
    parse_proc_meminfo(&contents)
}

fn read_arcstats() -> Option<ArcStats> {
    let contents = fs::read_to_string(ZFS_ARCSTATS_PATH).ok()?;
    parse_arcstats(&contents)
}

fn read_loadavg() -> Option<f64> {
    let contents = fs::read_to_string(PROC_LOADAVG_PATH).ok()?;
    parse_loadavg(&contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROC_STAT: &str = "\
cpu  10132153 290696 3084719 46828483 16683 0 25195 0 175628 0
cpu0 1393280 32966 572056 13343292 6130 0 17875 0 23933 0
intr 123456 0 0
";

    const SAMPLE_MEMINFO: &str = "\
MemTotal:       65712340 kB
MemFree:         2000000 kB
MemAvailable:   32856170 kB
Buffers:          500000 kB
Cached:         12000000 kB
SwapCached:       123456 kB
";

    const SAMPLE_MEMINFO_NO_AVAILABLE: &str = "\
MemTotal:       16777216 kB
MemFree:         2097152 kB
Buffers:         1048576 kB
Cached:          5242880 kB
SwapCached:      1048576 kB
";

    const SAMPLE_ARCSTATS: &str = "\
5 1 0x01 128 32768 123456789 987654321
name                            type data
hits                            4    12345678
misses                          4    876543
c_min                           4    1073741824
c_max                           4    34359738368
size                            4    12884901888
";

    #[test]
    fn parses_proc_stat_aggregate_line() {
        let times = parse_proc_stat(SAMPLE_PROC_STAT).expect("aggregate cpu line should parse");
        assert_eq!(
            times,
            CpuTimes {
                user: 10_132_153,
                nice: 290_696,
                system: 3_084_719,
                idle: 46_828_483,
                iowait: 16_683,
                irq: 0,
                softirq: 25_195,
                steal: 0,
            }
        );
    }

    #[test]
    fn parses_proc_stat_minimal_four_fields_and_ignores_per_core() {
        let only_per_core = "cpu0 100 20 30 400\n";
        assert_eq!(parse_proc_stat(only_per_core), None);

        let minimal = "cpu 100 20 30 400\n";
        let times = parse_proc_stat(minimal).expect("4-field cpu line should parse");
        assert_eq!(
            times,
            CpuTimes {
                user: 100,
                nice: 20,
                system: 30,
                idle: 400,
                iowait: 0,
                irq: 0,
                softirq: 0,
                steal: 0,
            }
        );

        assert_eq!(parse_proc_stat("cpu 100 20 bad 400\n"), None);
        assert_eq!(parse_proc_stat(""), None);
    }

    #[test]
    fn computes_cpu_percentage_idle_busy_and_zero_delta() {
        let base = CpuTimes {
            user: 100,
            nice: 0,
            system: 50,
            idle: 800,
            iowait: 50,
            irq: 0,
            softirq: 0,
            steal: 0,
        };

        // Zero delta must yield 0.0 (never NaN).
        let zero = compute_cpu_pct(base, base);
        assert_eq!(zero, 0.0);
        assert!(!zero.is_nan());

        // 100% idle delta (all 200 new ticks in idle + iowait).
        let all_idle = CpuTimes {
            idle: base.idle + 180,
            iowait: base.iowait + 20,
            ..base
        };
        assert_eq!(compute_cpu_pct(base, all_idle), 0.0);

        // 100% busy delta (all 200 new ticks in user + system + irq + steal).
        let all_busy = CpuTimes {
            user: base.user + 100,
            system: base.system + 50,
            irq: base.irq + 25,
            steal: base.steal + 25,
            ..base
        };
        assert_eq!(compute_cpu_pct(base, all_busy), 100.0);

        // 25% busy delta (50 busy + 150 idle = 200 total).
        let quarter = CpuTimes {
            user: base.user + 30,
            system: base.system + 20,
            idle: base.idle + 140,
            iowait: base.iowait + 10,
            ..base
        };
        let pct = compute_cpu_pct(base, quarter);
        assert!((pct - 25.0).abs() < 1e-9, "expected 25.0, got {pct}");

        // Counter regression must saturate safely to 0.0 without panicking.
        assert_eq!(compute_cpu_pct(all_busy, base), 0.0);
    }

    #[test]
    fn parses_proc_meminfo_with_and_without_mem_available() {
        let info = parse_proc_meminfo(SAMPLE_MEMINFO).expect("meminfo should parse");
        assert_eq!(info.mem_total_bytes, 65_712_340 * KIB);
        assert_eq!(info.mem_available_bytes, 32_856_170 * KIB);

        // Fallback when MemAvailable is absent: MemFree + Buffers + Cached (not SwapCached).
        let fallback =
            parse_proc_meminfo(SAMPLE_MEMINFO_NO_AVAILABLE).expect("fallback meminfo should parse");
        assert_eq!(fallback.mem_total_bytes, 16_777_216 * KIB);
        assert_eq!(
            fallback.mem_available_bytes,
            (2_097_152 + 1_048_576 + 5_242_880) * KIB
        );

        assert_eq!(parse_proc_meminfo("MemFree: 100 kB\n"), None);
        assert_eq!(parse_proc_meminfo("MemTotal: 100 kB\n"), None);
    }

    #[test]
    fn parses_zfs_arcstats() {
        let arc = parse_arcstats(SAMPLE_ARCSTATS).expect("arcstats should parse");
        assert_eq!(arc.size_bytes, 12_884_901_888);
        assert_eq!(arc.c_min_bytes, 1_073_741_824);
        assert_eq!(arc.reclaimable_bytes(), 11_811_160_064);

        assert_eq!(parse_arcstats("name type data\nsize 4 100\n"), None);
        assert_eq!(parse_arcstats(""), None);
    }

    #[test]
    fn computes_mem_usage_with_zfs_arc_scenarios() {
        let total = 64 * GIB;
        let available = 24 * GIB; // raw_used = 40 GiB
        let mem = MemInfo {
            mem_total_bytes: total,
            mem_available_bytes: available,
        };

        // Case 1: ZFS ARC size > c_min -> reclaimable ARC is subtracted from used RAM.
        let arc_large = ArcStats {
            size_bytes: 20 * GIB,
            c_min_bytes: 4 * GIB,
        };
        let usage = compute_mem_usage(mem, Some(arc_large));
        assert_eq!(usage.arc_bytes, Some(20 * GIB));
        assert_eq!(usage.reclaimable_arc_bytes, 16 * GIB);
        assert_eq!(usage.mem_used_bytes, 24 * GIB);
        assert_eq!(usage.mem_total_bytes, total);
        assert!((usage.mem_pct - 37.5).abs() < 1e-9);

        // Case 2: ZFS ARC size < c_min -> reclaimable saturates to 0, raw_used unchanged.
        let arc_small = ArcStats {
            size_bytes: 2 * GIB,
            c_min_bytes: 4 * GIB,
        };
        let usage_small = compute_mem_usage(mem, Some(arc_small));
        assert_eq!(usage_small.arc_bytes, Some(2 * GIB));
        assert_eq!(usage_small.reclaimable_arc_bytes, 0);
        assert_eq!(usage_small.mem_used_bytes, 40 * GIB);
        assert!((usage_small.mem_pct - 62.5).abs() < 1e-9);

        // Case 3: Missing arcstats (non-ZFS host) -> graceful None, 0 reclaimable.
        let usage_no_zfs = compute_mem_usage(mem, None);
        assert_eq!(usage_no_zfs.arc_bytes, None);
        assert_eq!(usage_no_zfs.reclaimable_arc_bytes, 0);
        assert_eq!(usage_no_zfs.mem_used_bytes, 40 * GIB);
        assert!((usage_no_zfs.mem_pct - 62.5).abs() < 1e-9);

        // Case 4: Reclaimable ARC exceeds raw_used -> mem_used saturates at 0, never underflows.
        let arc_huge = ArcStats {
            size_bytes: 50 * GIB,
            c_min_bytes: 2 * GIB,
        };
        let usage_saturated = compute_mem_usage(mem, Some(arc_huge));
        assert_eq!(usage_saturated.mem_used_bytes, 0);
        assert_eq!(usage_saturated.mem_pct, 0.0);

        // Case 5: Zero total memory -> 0.0% without NaN/division by zero.
        let zero_mem = MemInfo {
            mem_total_bytes: 0,
            mem_available_bytes: 0,
        };
        let usage_zero = compute_mem_usage(zero_mem, None);
        assert_eq!(usage_zero.mem_pct, 0.0);
        assert!(!usage_zero.mem_pct.is_nan());
    }

    #[test]
    fn parses_loadavg() {
        assert_eq!(parse_loadavg("0.42 0.38 0.31 2/1234 56789\n"), Some(0.42));
        assert_eq!(parse_loadavg("12.50 10.00 8.25 4/512 99"), Some(12.5));
        assert_eq!(parse_loadavg("0.00 0.00 0.00 1/100 1"), Some(0.0));
        assert_eq!(parse_loadavg(""), None);
        assert_eq!(parse_loadavg("NaN 0.0 0.0"), None);
        assert_eq!(parse_loadavg("-1.0 0.0 0.0"), None);
        assert_eq!(parse_loadavg("inf 0.0 0.0"), None);
    }

    #[test]
    fn formats_gib_and_mib() {
        assert_eq!(SysMon::format_gib(0), "0M");
        assert_eq!(SysMon::format_gib(512 * 1024 * 1024), "512M");
        assert_eq!(SysMon::format_gib(GIB - 1), "1.0G");
        assert_eq!(SysMon::format_gib(GIB), "1.0G");
        let bytes_18_4_gib = (18.4 * GIB as f64).round() as u64;
        assert_eq!(SysMon::format_gib(bytes_18_4_gib), "18.4G");
    }

    #[test]
    fn sysmon_initial_baseline_delta_and_ring_buffer_cap() {
        let mut mon = SysMon::with_baseline(None);
        assert!(mon.latest().is_none());
        assert!(mon.cpu_history().is_empty());
        assert!(mon.mem_history().is_empty());

        let t0 = CpuTimes {
            user: 100,
            nice: 0,
            system: 0,
            idle: 900,
            iowait: 0,
            irq: 0,
            softirq: 0,
            steal: 0,
        };
        let mem = MemInfo {
            mem_total_bytes: 100 * GIB,
            mem_available_bytes: 40 * GIB,
        };

        // First sample without a baseline establishes prev_cpu and reports 0.0% CPU.
        let s0 = mon.record_sample(Some(t0), Some(mem), None, Some(1.25));
        assert_eq!(s0.cpu_pct, 0.0);
        assert_eq!(s0.mem_pct, 60.0);
        assert_eq!(s0.load1, Some(1.25));
        assert_eq!(mon.latest(), Some(&s0));
        assert_eq!(mon.cpu_history().back(), Some(&0));
        assert_eq!(mon.mem_history().back(), Some(&60));

        // Second sample computes a real delta (50 busy / 100 total = 50%).
        let t1 = CpuTimes {
            user: 150,
            idle: 950,
            ..t0
        };
        let s1 = mon.record_sample(Some(t1), Some(mem), None, Some(1.5));
        assert!((s1.cpu_pct - 50.0).abs() < 1e-9);
        assert_eq!(mon.cpu_history().back(), Some(&50));

        // Fill past HISTORY_LEN and verify ring buffers stay capped and drop oldest entries.
        let mut curr = t1;
        for step in 0..(HISTORY_LEN + 25) {
            curr = CpuTimes {
                user: curr.user + 75,
                idle: curr.idle + 25,
                ..curr
            };
            let _ = mon.record_sample(Some(curr), Some(mem), None, Some(step as f64));
        }

        assert_eq!(mon.cpu_history().len(), HISTORY_LEN);
        assert_eq!(mon.mem_history().len(), HISTORY_LEN);
        // Every remaining CPU history entry should be 75 after oldest 0 and 50 were evicted.
        assert!(mon.cpu_history().iter().all(|&v| v == 75));
        assert!(mon.mem_history().iter().all(|&v| v == 60));
    }

    #[test]
    fn sysmon_live_proc_smoke_test() {
        let mut mon = SysMon::new();
        let sample = mon.sample();
        assert!((0.0..=100.0).contains(&sample.cpu_pct));
        assert!((0.0..=100.0).contains(&sample.mem_pct));
        assert!(sample.mem_total_bytes > 0);
        assert_eq!(mon.cpu_history().len(), 1);
        assert_eq!(mon.mem_history().len(), 1);
        assert_eq!(mon.latest(), Some(&sample));
    }
}
