//! Hook inflight sentinel files at `/run/nbb/inflight/<drv_hash>`.
//!
//! Written by `nbb-hook` when it accepts a candidate; unlinked on every
//! exit path. The controller's watchdog walks the directory once per tick
//! and calls `kill(pid, 0)`; a return of `ESRCH` means the hook process is
//! gone, the build is orphaned, and the admission must be retired.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bincode::{Decode, Encode};

use crate::protocol::frame::bincode_config;

#[derive(Encode, Decode, Clone, Debug, PartialEq, Eq)]
pub struct Sentinel {
    pub pid: u32,
    pub drv_path: String,
    pub admitted_at_ms: u64,
    pub predicted_ms: u64,
}

/// Filename for the sentinel of `drv_path`. The Nix store hash is the
/// natural unique key for `<hash>-<name>.drv`, so we use that.
pub fn drv_filename(drv_path: &str) -> String {
    let stem = drv_path.rsplit('/').next().unwrap_or(drv_path);
    let stem = stem.strip_suffix(".drv").unwrap_or(stem);
    let hash = stem.split('-').next().unwrap_or(stem);
    hash.to_string()
}

/// Atomically write a sentinel file under `dir`.
pub fn write_sentinel(dir: &Path, sentinel: &Sentinel) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let bytes = bincode::encode_to_vec(sentinel, bincode_config()).map_err(io::Error::other)?;
    let path = dir.join(drv_filename(&sentinel.drv_path));
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn read_sentinel(path: &Path) -> io::Result<Sentinel> {
    let bytes = fs::read(path)?;
    let (sentinel, _) =
        bincode::decode_from_slice(&bytes, bincode_config()).map_err(io::Error::other)?;
    Ok(sentinel)
}

/// `kill(pid, 0)` returns 0 if the process exists and `-1` with errno
/// `ESRCH` if not. Other errnos (e.g. `EPERM`) mean the process exists but
/// we can't signal it, which we still treat as alive.
pub fn pid_is_dead(pid: u32) -> bool {
    // SAFETY: `kill` with signal 0 only checks for the process; it sends no
    // signal and modifies no shared state. The pid is a plain integer.
    let result = unsafe { libc::kill(pid as i32, 0) };
    if result == 0 {
        return false;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drv_filename_extracts_hash() {
        assert_eq!(drv_filename("/nix/store/abc123-kwin-6.6.3.drv"), "abc123");
        assert_eq!(
            drv_filename("/nix/store/xyz0-cargo-package-syn-2.0.104.drv"),
            "xyz0"
        );
        assert_eq!(drv_filename("nohash.drv"), "nohash");
    }

    #[test]
    fn write_and_read_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "nbb-sentinel-{}-{}",
            std::process::id(),
            crate::util::now_ms()
        ));
        let s = Sentinel {
            pid: 12345,
            drv_path: "/nix/store/abc-foo-1.drv".to_string(),
            admitted_at_ms: 1000,
            predicted_ms: 5000,
        };
        let path = write_sentinel(&dir, &s).unwrap();
        let read = read_sentinel(&path).unwrap();
        assert_eq!(read, s);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pid_is_dead_recognizes_a_definitely_dead_pid() {
        // PID 1 is init and is always alive. A high PID we don't own is
        // almost certainly not in use; we can't guarantee, but PID
        // ~0x7FFF_FFFE is extremely unlikely to be a running process.
        assert!(!pid_is_dead(1));
        assert!(pid_is_dead(0x7FFF_FFFE));
    }
}
