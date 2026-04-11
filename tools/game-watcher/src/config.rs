use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_gpu_poll_interval")]
    pub gpu_poll_interval_ms: u64,
    #[serde(default)]
    pub games: Vec<Game>,
    #[serde(default)]
    pub gpu_guards: Vec<GpuGuard>,
}

fn default_poll_interval() -> u64 {
    1000
}
fn default_gpu_poll_interval() -> u64 {
    2000
}

#[derive(Debug, Deserialize)]
pub struct Game {
    pub name: String,
    pub app_id: u32,
    #[serde(default)]
    pub firewall: Vec<FirewallRule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FirewallRule {
    pub proto: Proto,
    pub port: u16,
    /// Optional inbound interface (e.g. "wg0"). Omit for global.
    #[serde(default)]
    pub interface: Option<String>,
    /// Generate an ip6tables rule in addition to iptables.
    #[serde(default = "default_true")]
    pub ipv6: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Udp,
    Tcp,
}

impl Proto {
    pub fn as_str(self) -> &'static str {
        match self {
            Proto::Udp => "udp",
            Proto::Tcp => "tcp",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GpuGuard {
    pub name: String,
    /// The guard is active whenever any of these games (by `name`) is running.
    pub requires_any_of: Vec<String>,
    pub service: String,
    pub gpu_util_threshold_pct: u32,
    pub settle_seconds: u64,
    pub action: GuardAction,
    #[serde(default)]
    pub escalate: Option<Escalate>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GuardAction {
    Restart,
    Stop,
}

#[derive(Debug, Deserialize)]
pub struct Escalate {
    pub when_triggers_exceed: usize,
    pub within_seconds: u64,
    pub action: GuardAction,
    /// Escalation only applies if any of these games (by `name`) is running.
    pub applies_if_any_of: Vec<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config file {}", path.display()))?;
        Ok(cfg)
    }
}
