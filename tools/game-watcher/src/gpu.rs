use anyhow::{Context, Result};
use nvml_wrapper::Nvml;
use std::process::Command;
use tracing::{debug, warn};

pub struct GpuMonitor {
    _nvml: Nvml,
    // We reinitialise the Device each sample to sidestep lifetime tangles —
    // cheap enough at 0.5 Hz.
    last_seen_us: u64,
}

impl GpuMonitor {
    pub fn new() -> Result<Self> {
        let nvml = Nvml::init().context("initialising NVML (is libnvidia-ml.so on the loader path?)")?;
        Ok(Self {
            _nvml: nvml,
            last_seen_us: 0,
        })
    }

    /// Sample the maximum SM utilisation across all samples belonging to `pids`.
    /// Returns None if no samples were available for any listed PID this tick.
    pub fn sample_max_util(&mut self, pids: &[u32]) -> Result<Option<u32>> {
        if pids.is_empty() {
            return Ok(None);
        }
        let device = self
            ._nvml
            .device_by_index(0)
            .context("getting NVML device 0")?;
        let samples = match device.process_utilization_stats(self.last_seen_us) {
            Ok(s) => s,
            // NotFound simply means no new samples since last_seen_us.
            Err(nvml_wrapper::error::NvmlError::NotFound) => return Ok(None),
            Err(e) => return Err(e).context("reading process utilization stats"),
        };

        let mut max_util = 0u32;
        let mut matched = false;
        for sample in &samples {
            if sample.timestamp > self.last_seen_us {
                self.last_seen_us = sample.timestamp;
            }
            if pids.contains(&sample.pid) {
                matched = true;
                if sample.sm_util > max_util {
                    max_util = sample.sm_util;
                }
            }
        }
        if matched {
            Ok(Some(max_util))
        } else {
            Ok(None)
        }
    }
}

/// Query systemd for the MainPID of a service. Returns None if the service
/// is not running (MainPID is 0 or the command fails).
pub fn service_main_pid(service: &str) -> Result<Option<u32>> {
    let output = Command::new("systemctl")
        .args(["show", "-p", "MainPID", "--value", service])
        .output()
        .with_context(|| format!("systemctl show -p MainPID {service}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%service, %stderr, "systemctl show failed");
        return Ok(None);
    }
    let pid_str = String::from_utf8_lossy(&output.stdout);
    let pid: u32 = pid_str.trim().parse().unwrap_or(0);
    if pid == 0 {
        debug!(%service, "service not running (MainPID=0)");
        Ok(None)
    } else {
        Ok(Some(pid))
    }
}
