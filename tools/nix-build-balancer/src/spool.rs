//! Spool-file writer used by `nbb-event`.
//!
//! Writes one bincode-encoded [`SpoolEvent`] to `<spool_dir>/<ulid>.evt.tmp`,
//! fsyncs, then atomically renames to `<ulid>.evt`. The agent's spool
//! watcher reads `.evt` files in ULID order (which is time order).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::protocol::frame::bincode_config;
use crate::protocol::ops::SpoolEvent;

pub fn write_event(spool_dir: &Path, event: &SpoolEvent) -> io::Result<PathBuf> {
    fs::create_dir_all(spool_dir)?;
    let bytes = bincode::encode_to_vec(event, bincode_config()).map_err(io::Error::other)?;
    let id = ulid::Ulid::new().to_string();
    let final_path = spool_dir.join(format!("{id}.evt"));
    let tmp_path = spool_dir.join(format!("{id}.evt.tmp"));
    {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, &final_path)?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ops::{BuildStatus, SpoolEvent};
    use crate::util::now_ms;

    #[test]
    fn write_event_creates_evt_file() {
        let dir = std::env::temp_dir().join(format!(
            "nbb-spool-test-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let event = SpoolEvent::Start {
            drv_path: "/nix/store/abc-foo.drv".to_string(),
            pname: "foo".to_string(),
            host: "tsugumi".to_string(),
            ts_ms: 1000,
        };
        let path = write_event(&dir, &event).unwrap();
        assert!(path.exists());
        assert_eq!(
            path.extension().and_then(|s| s.to_str()),
            Some("evt"),
            "atomic rename should leave only .evt, no .tmp"
        );

        // Verify decode round-trip.
        let bytes = fs::read(&path).unwrap();
        let (decoded, _) =
            bincode::decode_from_slice::<SpoolEvent, _>(&bytes, bincode_config()).unwrap();
        assert_eq!(decoded, event);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finish_event_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("nbb-spool-fin-{}-{}", std::process::id(), now_ms()));
        let event = SpoolEvent::Finish {
            drv_path: "/nix/store/abc-foo.drv".to_string(),
            pname: "foo".to_string(),
            host: "tsugumi".to_string(),
            ts_ms: 2000,
            status: BuildStatus::Failure,
            out_paths: vec!["/nix/store/out".to_string()],
        };
        let path = write_event(&dir, &event).unwrap();
        let bytes = fs::read(&path).unwrap();
        let (decoded, _) =
            bincode::decode_from_slice::<SpoolEvent, _>(&bytes, bincode_config()).unwrap();
        assert_eq!(decoded, event);
        let _ = fs::remove_dir_all(&dir);
    }
}
