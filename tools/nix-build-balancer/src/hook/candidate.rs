//! Nix build-hook protocol parsing — ported from `old-src/src/hook/candidate.rs`.
//!
//! The Nix build-hook reads:
//!   - A `(name, value)*` settings stream terminated by a zero-length key.
//!   - A series of `try` candidates, each: `am_willing`, `needed_system`,
//!     `drv_path`, `required_features`. EOF or any non-"try" opcode ends
//!     the loop.

use std::io::{self, Read, Write};

use crate::nix_protocol::{
    read_nix_string, read_nix_strings, read_nix_u64, write_nix_string, write_nix_strings,
    write_nix_u64,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookCandidate {
    pub am_willing: u64,
    pub needed_system: String,
    pub drv_path: String,
    pub required_features: Vec<String>,
}

/// Read the initial key/value setting stream from Nix's build-hook protocol.
pub fn read_hook_settings<R: Read>(reader: &mut R) -> io::Result<Vec<(String, String)>> {
    let mut settings = Vec::new();
    loop {
        if read_nix_u64(reader)? == 0 {
            break;
        }
        let name = read_nix_string(reader)?;
        let value = read_nix_string(reader)?;
        settings.push((name, value));
    }
    Ok(settings)
}

/// Read one build candidate. Returns `Ok(None)` on EOF or any non-`try` op.
pub fn read_hook_candidate<R: Read>(reader: &mut R) -> io::Result<Option<HookCandidate>> {
    let op = match read_nix_string(reader) {
        Ok(op) => op,
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    };
    if op != "try" {
        return Ok(None);
    }
    let am_willing = read_nix_u64(reader)?;
    let needed_system = read_nix_string(reader)?;
    let drv_path = read_nix_string(reader)?;
    let required_features = read_nix_strings(reader)?;
    Ok(Some(HookCandidate {
        am_willing,
        needed_system,
        drv_path,
        required_features,
    }))
}

/// Pass settings through to a delegated `nix __build-remote` child, with
/// the `builders` value replaced by the controller-supplied builder line.
pub fn write_hook_settings<W: Write>(
    writer: &mut W,
    settings: &[(String, String)],
    builder_line: &str,
) -> io::Result<()> {
    for (name, value) in settings {
        if name == "builders" {
            // Skip — we override below.
            continue;
        }
        write_nix_u64(writer, 1)?;
        write_nix_string(writer, name)?;
        write_nix_string(writer, value)?;
    }
    write_nix_u64(writer, 1)?;
    write_nix_string(writer, "builders")?;
    write_nix_string(writer, builder_line)?;
    write_nix_u64(writer, 0)
}

/// Write a `try` candidate for a delegated child.
pub fn write_hook_candidate<W: Write>(writer: &mut W, candidate: &HookCandidate) -> io::Result<()> {
    write_nix_string(writer, "try")?;
    write_nix_u64(writer, candidate.am_willing)?;
    write_nix_string(writer, &candidate.needed_system)?;
    write_nix_string(writer, &candidate.drv_path)?;
    write_nix_strings(writer, &candidate.required_features)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_round_trip() {
        let candidate = HookCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-kwin-6.6.3.drv".to_string(),
            required_features: vec!["kvm".to_string(), "big-parallel".to_string()],
        };
        let mut bytes = Vec::new();
        write_hook_candidate(&mut bytes, &candidate).unwrap();
        let mut cursor = std::io::Cursor::new(&bytes);
        let parsed = read_hook_candidate(&mut cursor).unwrap().unwrap();
        assert_eq!(parsed, candidate);
    }

    #[test]
    fn settings_round_trip_replaces_builders() {
        let settings = vec![
            ("max-jobs".to_string(), "8".to_string()),
            ("builders".to_string(), "old-line".to_string()),
        ];
        let mut bytes = Vec::new();
        write_hook_settings(
            &mut bytes,
            &settings,
            "ssh-ng://tsugumi x86_64-linux - 16 1 - - -",
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(&bytes);
        let parsed = read_hook_settings(&mut cursor).unwrap();
        // The old "builders" should be replaced by the new builder_line.
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().any(|(k, v)| k == "max-jobs" && v == "8"));
        assert!(parsed
            .iter()
            .any(|(k, v)| k == "builders" && v.starts_with("ssh-ng://tsugumi")));
    }

    #[test]
    fn read_hook_candidate_returns_none_on_eof() {
        let bytes: Vec<u8> = Vec::new();
        let mut cursor = std::io::Cursor::new(&bytes);
        let result = read_hook_candidate(&mut cursor).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn read_hook_candidate_returns_none_on_non_try_op() {
        // Encode op = "stop"
        let mut bytes = Vec::new();
        write_nix_string(&mut bytes, "stop").unwrap();
        let mut cursor = std::io::Cursor::new(&bytes);
        let result = read_hook_candidate(&mut cursor).unwrap();
        assert!(result.is_none());
    }
}
