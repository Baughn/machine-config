use std::fs;
use std::io;
use std::path::Path;
use std::thread;

use crate::api::client::get_http_tcp;
use crate::api::types::{PackageStats, StatsResponse, Telemetry};
use crate::config::Config;
use crate::util::json_error;

/// Read cached telemetry for a remote host from the controller data directory.
pub fn read_remote_telemetry(data_dir: &Path, remote_host: &str) -> io::Result<Telemetry> {
    let text = fs::read_to_string(data_dir.join(format!("telemetry-{remote_host}.json")))?;
    let mut telemetry: Telemetry = serde_json::from_str(&text).map_err(json_error)?;
    if telemetry.host.is_empty() {
        telemetry.host = remote_host.to_string();
    }
    Ok(telemetry)
}

/// Read one package's latest cached stats from a polled remote agent.
pub fn remote_package_stats(
    data_dir: &Path,
    remote_host: &str,
    pname: &str,
) -> io::Result<Option<PackageStats>> {
    let path = data_dir.join(format!("stats-{remote_host}.json"));
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    Ok(parse_package_stats(&text, pname))
}

/// Extract one package's scheduler-facing stats from `/stats` JSON.
fn parse_package_stats(text: &str, pname: &str) -> Option<PackageStats> {
    let stats: StatsResponse = serde_json::from_str(text).ok()?;
    stats
        .packages
        .into_iter()
        .find(|stats| stats.pname == pname)
        .map(|stats| PackageStats {
            count: stats.count,
            p95_ms: stats.p95_ms,
        })
}

/// Poll configured remote agents and cache their latest telemetry and stats.
pub fn poll_remotes(cfg: Config) {
    loop {
        for remote in &cfg.remote {
            let Some((name, addr)) = remote.split_once('=') else {
                eprintln!("invalid remote {remote}, expected name=addr:port");
                continue;
            };
            match get_http_tcp(addr, "/telemetry") {
                Ok(body) => {
                    let path = cfg.data_dir.join(format!("telemetry-{name}.json"));
                    if let Err(err) = fs::write(path, body) {
                        eprintln!("writing remote telemetry failed: {err}");
                    }
                }
                Err(err) => eprintln!("polling {name} at {addr} failed: {err}"),
            }
            match get_http_tcp(addr, "/stats") {
                Ok(body) => {
                    let path = cfg.data_dir.join(format!("stats-{name}.json"));
                    if let Err(err) = fs::write(path, body) {
                        eprintln!("writing remote stats failed: {err}");
                    }
                }
                Err(err) => eprintln!("polling stats for {name} at {addr} failed: {err}"),
            }
        }
        thread::sleep(cfg.poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_stats_from_remote_json() {
        let stats = r#"{"unknown_p95_ms":1800000,"packages":[{"pname":"kwin","count":3,"p50_ms":10,"p80_ms":20,"p95_ms":30}]}"#;
        let parsed = parse_package_stats(stats, "kwin").unwrap();
        assert_eq!(parsed.count, 3);
        assert_eq!(parsed.p95_ms, 30);
        assert!(parse_package_stats(stats, "missing").is_none());
    }
}
