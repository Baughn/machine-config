use procfs::{Current, Meminfo, MemoryPressure};
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;

use crate::util::now_ms;

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

/// One agent-side telemetry snapshot, returned across the wire to the
/// controller in response to `TELEMETRY_GET`.
///
/// Note: this struct intentionally does not carry CPU busy ratio or split
/// local/remote slot counts. The rewrite collapses both — admissions are the
/// sole load signal, and `nix_slots_active` is reported only for divergence
/// observability.
#[derive(Clone, Debug, PartialEq)]
pub struct Telemetry {
    pub mem_available_kb: u64,
    pub psi_memory_some_avg10: Option<f64>,
    pub nix_slots_active: usize,
    pub sampled_at_ms: u128,
}

pub fn sample() -> io::Result<Telemetry> {
    Ok(Telemetry {
        mem_available_kb: read_mem_available_kb()?,
        psi_memory_some_avg10: read_psi_memory_some_avg10().ok().flatten(),
        nix_slots_active: count_active_nix_slots(SLOT_DIR),
        sampled_at_ms: now_ms(),
    })
}

const SLOT_DIR: &str = "/nix/var/nix/current-load";

fn read_mem_available_kb() -> io::Result<u64> {
    let meminfo = Meminfo::current().map_err(io::Error::other)?;
    Ok(meminfo.mem_available.map(|v| v / 1024).unwrap_or(0))
}

fn read_psi_memory_some_avg10() -> io::Result<Option<f64>> {
    let pressure = MemoryPressure::current().map_err(io::Error::other)?;
    Ok(Some(pressure.some.avg10.into()))
}

/// Count flock-held Nix build slot files in `dir`. Unlocked files are stale.
fn count_active_nix_slots<P: AsRef<Path>>(dir: P) -> usize {
    let Ok(entries) = fs::read_dir(dir.as_ref()) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "main-lock" || name.ends_with(".upload-lock") {
            continue;
        }
        if slot_file_is_locked(&entry.path()) {
            total += 1;
        }
    }
    total
}

fn slot_file_is_locked(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    let fd = file.as_raw_fd();
    if try_flock_exclusive(fd) {
        let _ = unlock_flock(fd);
        false
    } else {
        true
    }
}

fn try_flock_exclusive(fd: RawFd) -> bool {
    // SAFETY: `fd` comes from a live `File` in `slot_file_is_locked`, and
    // `flock` does not take ownership of the descriptor.
    unsafe { flock(fd, LOCK_EX | LOCK_NB) == 0 }
}

fn unlock_flock(fd: RawFd) -> io::Result<()> {
    // SAFETY: `fd` comes from a live `File` in `slot_file_is_locked`, and
    // `flock` does not take ownership of the descriptor.
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
    fn stale_slot_files_are_not_active() {
        let dir =
            std::env::temp_dir().join(format!("nbb-slot-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ssh-ng:__svein@tsugumi.local-0");
        fs::write(&path, "").unwrap();

        assert!(!slot_file_is_locked(&path));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_slot_dir_yields_zero() {
        let path =
            std::env::temp_dir().join(format!("nbb-missing-{}-{}", std::process::id(), now_ms()));
        assert_eq!(count_active_nix_slots(&path), 0);
    }

    #[test]
    fn empty_slot_dir_yields_zero() {
        let dir =
            std::env::temp_dir().join(format!("nbb-empty-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(count_active_nix_slots(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn upload_and_main_locks_are_ignored() {
        let dir =
            std::env::temp_dir().join(format!("nbb-ignored-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&dir).unwrap();
        // Even though we don't actually lock these, the names are filtered
        // before we check the lock state, so they would be filtered anyway.
        fs::write(dir.join("main-lock"), "").unwrap();
        fs::write(dir.join("foo.upload-lock"), "").unwrap();
        assert_eq!(count_active_nix_slots(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }
}
