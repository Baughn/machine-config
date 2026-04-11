use std::collections::{HashMap, HashSet};
use std::fs;

/// Stop events must persist for this many consecutive scans before firing,
/// to tolerate Steam briefly reparenting processes during startup.
const STOP_DEBOUNCE_TICKS: u8 = 2;

pub struct ActiveGames {
    current: HashSet<u32>,
    pending_stop: HashMap<u32, u8>,
}

impl ActiveGames {
    pub fn new() -> Self {
        Self {
            current: HashSet::new(),
            pending_stop: HashMap::new(),
        }
    }

    /// Returns (started, stopped) AppIds for this tick.
    pub fn tick(&mut self, scanned: HashSet<u32>) -> (Vec<u32>, Vec<u32>) {
        let mut started = Vec::new();
        let mut stopped = Vec::new();

        for &app_id in &scanned {
            if !self.current.contains(&app_id) {
                started.push(app_id);
                self.current.insert(app_id);
            }
            // Any pending stop for this AppId is cancelled — it's back.
            self.pending_stop.remove(&app_id);
        }

        let absent: Vec<u32> = self
            .current
            .iter()
            .copied()
            .filter(|id| !scanned.contains(id))
            .collect();

        for app_id in absent {
            let ticks = self.pending_stop.entry(app_id).or_insert(0);
            *ticks += 1;
            if *ticks >= STOP_DEBOUNCE_TICKS {
                self.current.remove(&app_id);
                self.pending_stop.remove(&app_id);
                stopped.push(app_id);
            }
        }

        (started, stopped)
    }
}

/// Scan /proc for processes whose cmdline contains `SteamLaunch AppId=<n>`.
pub fn scan_proc() -> HashSet<u32> {
    let mut ids = HashSet::new();
    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return ids,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let cmdline_path = entry.path().join("cmdline");
        let Ok(bytes) = fs::read(&cmdline_path) else {
            continue;
        };
        if let Some(app_id) = extract_app_id(&bytes) {
            ids.insert(app_id);
        }
    }
    ids
}

/// Parse a null-separated cmdline looking for `SteamLaunch` followed by `AppId=<n>`.
pub fn extract_app_id(cmdline: &[u8]) -> Option<u32> {
    // Args are null-separated. Split into &[u8] slices.
    let args: Vec<&[u8]> = cmdline.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
    let mut saw_launch = false;
    for arg in &args {
        if !saw_launch {
            if *arg == b"SteamLaunch" {
                saw_launch = true;
            }
            continue;
        }
        if let Some(rest) = arg.strip_prefix(b"AppId=") {
            let s = std::str::from_utf8(rest).ok()?;
            return s.parse::<u32>().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reaper_cmdline() {
        let cmd = b"reaper\0SteamLaunch\0AppId=544550\0--\0/path/to/game\0";
        assert_eq!(extract_app_id(cmd), Some(544550));
    }

    #[test]
    fn ignores_unrelated_cmdline() {
        let cmd = b"firefox\0--new-tab\0";
        assert_eq!(extract_app_id(cmd), None);
    }

    #[test]
    fn requires_launch_before_appid() {
        let cmd = b"reaper\0AppId=544550\0";
        assert_eq!(extract_app_id(cmd), None);
    }

    #[test]
    fn debounces_stop() {
        let mut a = ActiveGames::new();
        let mut set = HashSet::new();
        set.insert(544550);
        assert_eq!(a.tick(set.clone()), (vec![544550], vec![]));
        // Missing once — not yet stopped.
        let (started, stopped) = a.tick(HashSet::new());
        assert!(started.is_empty());
        assert!(stopped.is_empty());
        // Missing twice — fires.
        let (_, stopped) = a.tick(HashSet::new());
        assert_eq!(stopped, vec![544550]);
    }

    #[test]
    fn reappears_cancels_pending_stop() {
        let mut a = ActiveGames::new();
        let mut set = HashSet::new();
        set.insert(544550);
        a.tick(set.clone());
        a.tick(HashSet::new()); // pending
        let (started, stopped) = a.tick(set);
        assert!(started.is_empty());
        assert!(stopped.is_empty());
    }
}
