use procfs::{Current, CurrentSI, KernelStats, Meminfo, MemoryPressure};
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::api::types::Telemetry;
use crate::util::{now_ms, proc_error};

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

/// Sample the local host telemetry used by agents and scheduler decisions.
pub fn read_telemetry(host: &str) -> io::Result<Telemetry> {
    let (cpu_busy_ratio, _) = read_cpu_busy_ratio()?;
    let (mem_total_kb, mem_available_kb) = read_meminfo()?;
    let psi_memory_some_avg10 = read_psi_memory_some_avg10().ok().flatten();
    let (nix_slots_total, nix_slots_local, nix_slots_remote) = read_nix_slots();

    Ok(Telemetry {
        host: host.to_string(),
        timestamp_ms: now_ms(),
        cpu_busy_ratio,
        mem_total_kb,
        mem_available_kb,
        psi_memory_some_avg10,
        nix_slots_total,
        nix_slots_local,
        nix_slots_remote,
    })
}

#[derive(Debug)]
struct CpuSample {
    idle: u64,
    total: u64,
}

fn read_cpu_busy_ratio() -> io::Result<(Option<f64>, CpuSample)> {
    let first = read_cpu_sample()?;
    thread::sleep(Duration::from_millis(100));
    let second = read_cpu_sample()?;
    let total_delta = second.total.saturating_sub(first.total);
    let idle_delta = second.idle.saturating_sub(first.idle);
    let busy = if total_delta == 0 {
        None
    } else {
        Some(1.0 - (idle_delta as f64 / total_delta as f64))
    };
    Ok((busy, second))
}

fn read_cpu_sample() -> io::Result<CpuSample> {
    let cpu = KernelStats::current().map_err(proc_error)?.total;
    let idle = cpu.idle + cpu.iowait.unwrap_or(0);
    let total = cpu.user
        + cpu.nice
        + cpu.system
        + cpu.idle
        + cpu.iowait.unwrap_or(0)
        + cpu.irq.unwrap_or(0)
        + cpu.softirq.unwrap_or(0)
        + cpu.steal.unwrap_or(0)
        + cpu.guest.unwrap_or(0)
        + cpu.guest_nice.unwrap_or(0);
    Ok(CpuSample { idle, total })
}

fn read_meminfo() -> io::Result<(Option<u64>, Option<u64>)> {
    let meminfo = Meminfo::current().map_err(proc_error)?;
    Ok((
        Some(meminfo.mem_total / 1024),
        meminfo.mem_available.map(|v| v / 1024),
    ))
}

fn read_psi_memory_some_avg10() -> io::Result<Option<f64>> {
    let pressure = MemoryPressure::current().map_err(proc_error)?;
    Ok(Some(pressure.some.avg10.into()))
}

/// Count currently locked Nix build slot files.
///
/// Nix represents active local and remote builds as locked files in
/// `/nix/var/nix/current-load`; unlocked files are stale and ignored.
fn read_nix_slots() -> (usize, usize, usize) {
    let dir = Path::new("/nix/var/nix/current-load");
    let mut total = 0;
    let mut local = 0;
    let mut remote = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0, 0);
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "main-lock" || name.ends_with(".upload-lock") {
            continue;
        }
        if !slot_file_is_locked(&entry.path()) {
            continue;
        }
        total += 1;
        if name.contains("localhost") {
            local += 1;
        } else if name.starts_with("ssh") {
            remote += 1;
        }
    }
    (total, local, remote)
}

fn slot_file_is_locked(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    let fd = file.as_raw_fd();
    let result = try_flock_exclusive(fd);
    if result {
        let _ = unlock_flock(fd);
        false
    } else {
        true
    }
}

fn try_flock_exclusive(fd: RawFd) -> bool {
    // SAFETY: `fd` comes from a live `File` in `slot_file_is_locked`, and `flock`
    // does not take ownership of the descriptor.
    unsafe { flock(fd, LOCK_EX | LOCK_NB) == 0 }
}

fn unlock_flock(fd: RawFd) -> io::Result<()> {
    // SAFETY: `fd` comes from a live `File` in `slot_file_is_locked`, and `flock`
    // does not take ownership of the descriptor.
    if unsafe { flock(fd, LOCK_UN) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_slot_files_are_not_active_slots() {
        let dir = std::env::temp_dir().join(format!(
            "nbb-slot-{}-{}",
            std::process::id(),
            crate::util::now_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ssh-ng:__svein@tsugumi.local-0");
        fs::write(&path, "").unwrap();

        assert!(!slot_file_is_locked(&path));
        let _ = fs::remove_dir_all(dir);
    }
}
