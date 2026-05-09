use std::io::{self, Read, Write};

use crate::api::types::BuildCandidate;
use crate::config::{DEFAULT_REMOTE_HOST, DEFAULT_REMOTE_STORE_URI};
use crate::nix_protocol::{
    read_nix_string, read_nix_strings, read_nix_u64, write_nix_string, write_nix_strings,
    write_nix_u64,
};
use crate::util::pname_from_drv;

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

/// Read one build candidate from the Nix build-hook protocol.
pub fn read_hook_candidate<R: Read>(reader: &mut R) -> io::Result<Option<BuildCandidate>> {
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
    let pname = pname_from_drv(&drv_path);
    Ok(Some(BuildCandidate {
        am_willing,
        needed_system,
        drv_path,
        required_features,
        pname,
        remote_host: DEFAULT_REMOTE_HOST.to_string(),
        remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
    }))
}

/// Write hook settings to a delegated `nix __build-remote` child.
pub fn write_hook_settings<W: Write>(
    writer: &mut W,
    settings: &[(String, String)],
    remote_builder: &str,
) -> io::Result<()> {
    for (name, value) in settings {
        write_nix_u64(writer, 1)?;
        write_nix_string(writer, name)?;
        write_nix_string(writer, value)?;
    }
    write_nix_u64(writer, 1)?;
    write_nix_string(writer, "builders")?;
    write_nix_string(writer, remote_builder)?;
    write_nix_u64(writer, 0)
}

/// Write a `try` build candidate in Nix's build-hook protocol format.
pub fn write_hook_candidate<W: Write>(
    writer: &mut W,
    candidate: &BuildCandidate,
) -> io::Result<()> {
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
    fn nix_protocol_round_trips_candidate() {
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-kwin-6.6.3.drv".to_string(),
            required_features: vec!["kvm".to_string(), "big-parallel".to_string()],
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };
        let settings = vec![("builders".to_string(), "ssh-ng://builder".to_string())];
        let mut bytes = Vec::new();
        write_hook_settings(
            &mut bytes,
            &settings,
            "ssh-ng://tsugumi x86_64-linux - 1 1 - - -",
        )
        .unwrap();
        write_hook_candidate(&mut bytes, &candidate).unwrap();

        let mut cursor = std::io::Cursor::new(bytes);
        let parsed_settings = read_hook_settings(&mut cursor).unwrap();
        let parsed = read_hook_candidate(&mut cursor).unwrap().unwrap();

        assert_eq!(parsed_settings.len(), 2);
        assert_eq!(parsed.drv_path, candidate.drv_path);
        assert_eq!(parsed.pname, "kwin");
        assert_eq!(parsed.required_features, candidate.required_features);
    }
}
