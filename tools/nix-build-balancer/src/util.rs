use std::env;
use std::fs;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn hostname_fallback() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn invalid<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

pub fn timestamp_to_i64(value: u128) -> io::Result<i64> {
    i64::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "timestamp is too large"))
}

pub fn duration_to_i64(value: u128) -> io::Result<i64> {
    i64::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "duration is too large"))
}

pub fn sqlite_error(err: rusqlite::Error) -> io::Error {
    io::Error::other(err)
}

pub fn proc_error(err: procfs::ProcError) -> io::Error {
    io::Error::other(err)
}

pub fn json_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

/// Extract a normalized package name from a Nix derivation path.
///
/// The store hash and `.drv` suffix are removed before version-like suffix
/// components are stripped.
pub fn pname_from_drv(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .strip_suffix(".drv")
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
    let without_hash = name.split_once('-').map(|(_, rest)| rest).unwrap_or(name);
    normalize_pname(without_hash)
}

fn normalize_pname(name: &str) -> String {
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() <= 1 {
        return name.to_string();
    }

    let mut end = parts.len();
    while end > 1 && looks_versionish(parts[end - 1]) {
        end -= 1;
    }
    parts[..end].join("-")
}

fn looks_versionish(part: &str) -> bool {
    part.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || part
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '.' || ch == '_' || ch == '+')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_basic_pnames() {
        assert_eq!(pname_from_drv("/nix/store/hash-kwin-6.6.3.drv"), "kwin");
        assert_eq!(
            pname_from_drv("/nix/store/hash-cargo-package-syn-2.0.104.drv"),
            "cargo-package-syn"
        );
        assert_eq!(
            pname_from_drv("/nix/store/hash-system-units.drv"),
            "system-units"
        );
        assert_eq!(
            pname_from_drv("/nix/store/hash-linux-6.19.5-3.drv"),
            "linux"
        );
    }
}
