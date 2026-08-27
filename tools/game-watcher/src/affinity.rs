//! CPU-affinity pinning for game process trees.
//!
//! Games are pinned by walking the Steam reaper's descendant tree via
//! `/proc/<pid>/task/<tid>/children` and calling `sched_setaffinity` on every
//! thread. Enforcement is re-run each poll tick: it is idempotent, and repeat
//! application catches threads spawned after the initial pin. Affinity is
//! inherited across fork/clone, so anything spawned by an already-pinned task
//! stays pinned even if it later reparents out of the tree.

use anyhow::{bail, Context, Result};
use nix::sched::{sched_setaffinity, CpuSet};
use nix::unistd::Pid;
use std::fs;

/// Parse a Linux cpulist string (e.g. `"0-7,16-23"`) into a `CpuSet`.
///
/// # Errors
///
/// Fails on empty elements, non-numeric CPUs, reversed ranges, or CPU numbers
/// beyond the platform's `CpuSet` capacity.
pub fn parse_cpulist(list: &str) -> Result<CpuSet> {
    let mut set = CpuSet::new();
    for part in list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            bail!("empty element in cpulist '{list}'");
        }
        let (lo, hi) = match part.split_once('-') {
            Some((a, b)) => (parse_cpu(a, list)?, parse_cpu(b, list)?),
            None => {
                let n = parse_cpu(part, list)?;
                (n, n)
            }
        };
        if lo > hi {
            bail!("reversed range '{part}' in cpulist '{list}'");
        }
        for cpu in lo..=hi {
            set.set(cpu)
                .with_context(|| format!("cpu {cpu} out of range in cpulist '{list}'"))?;
        }
    }
    Ok(set)
}

fn parse_cpu(s: &str, list: &str) -> Result<usize> {
    s.parse::<usize>()
        .with_context(|| format!("bad cpu '{s}' in cpulist '{list}'"))
}

/// Pin every thread in `root`'s process tree (including `root` itself) to
/// `set`. Returns the number of threads pinned. Tasks that exit mid-walk are
/// skipped silently — races with process teardown are routine here.
pub fn apply_to_tree(root: u32, set: &CpuSet) -> usize {
    let mut pinned = 0;
    for pid in descendants(root) {
        for tid in tasks_of(pid) {
            if sched_setaffinity(Pid::from_raw(tid as i32), set).is_ok() {
                pinned += 1;
            }
        }
    }
    pinned
}

/// `root` plus all descendant PIDs, discovered breadth-first through
/// `/proc/<pid>/task/<tid>/children`.
fn descendants(root: u32) -> Vec<u32> {
    let mut result = vec![root];
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        for tid in tasks_of(pid) {
            let path = format!("/proc/{pid}/task/{tid}/children");
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let children = text
                .split_whitespace()
                .filter_map(|s| s.parse::<u32>().ok());
            for child in children {
                result.push(child);
                queue.push(child);
            }
        }
    }
    result
}

/// Thread IDs of `pid`, from `/proc/<pid>/task`. Empty if the process is gone.
fn tasks_of(pid: u32) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpus_in(set: &CpuSet) -> Vec<usize> {
        (0..CpuSet::count())
            .filter(|&c| set.is_set(c).unwrap_or(false))
            .collect()
    }

    #[test]
    fn parses_single_cpu() {
        let set = parse_cpulist("3").unwrap();
        assert_eq!(cpus_in(&set), vec![3]);
    }

    #[test]
    fn parses_ranges_and_lists() {
        let set = parse_cpulist("0-2,7,16-17").unwrap();
        assert_eq!(cpus_in(&set), vec![0, 1, 2, 7, 16, 17]);
    }

    #[test]
    fn tolerates_whitespace() {
        let set = parse_cpulist(" 0-1 , 4 ").unwrap();
        assert_eq!(cpus_in(&set), vec![0, 1, 4]);
    }

    #[test]
    fn rejects_reversed_range() {
        assert!(parse_cpulist("7-0").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_cpulist("").is_err());
        assert!(parse_cpulist("0,,2").is_err());
        assert!(parse_cpulist("banana").is_err());
        assert!(parse_cpulist("1-").is_err());
    }

    #[test]
    fn pins_own_process_tree() {
        // Pin ourselves to CPU 0 and back — exercises the /proc walk and the
        // syscall path without touching any other process.
        let me = std::process::id();
        let original = nix::sched::sched_getaffinity(Pid::from_raw(0)).unwrap();
        let one = parse_cpulist("0").unwrap();
        assert!(apply_to_tree(me, &one) >= 1);
        let now = nix::sched::sched_getaffinity(Pid::from_raw(0)).unwrap();
        assert_eq!(cpus_in(&now), vec![0]);
        apply_to_tree(me, &original);
    }
}
