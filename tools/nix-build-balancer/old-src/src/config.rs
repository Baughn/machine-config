use clap::ValueEnum;
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_REMOTE_HOST: &str = "tsugumi";
pub const DEFAULT_REMOTE_STORE_URI: &str = "ssh-ng://svein@tsugumi.local";
pub const DEFAULT_REMOTE_BUILDER: &str =
    "ssh-ng://svein@tsugumi.local x86_64-linux /home/svein/.ssh/id_ed25519 16 1 nixos-test,kvm,big-parallel - -";
pub const DEFAULT_STALE_START_MS: u128 = 24 * 60 * 60 * 1000;
pub const DEFAULT_MAX_SAMPLES_PER_PNAME: usize = 200;

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub host: String,
    pub data_dir: PathBuf,
    pub unix_socket: Option<PathBuf>,
    pub listen: Option<String>,
    pub remote: Vec<String>,
    pub poll_interval: Duration,
    pub max_samples_per_pname: usize,
    pub stale_start_ms: u128,
    pub once: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Mode {
    Agent,
    Controller,
}

pub fn default_remote_host() -> String {
    DEFAULT_REMOTE_HOST.to_string()
}

pub fn default_remote_store_uri() -> String {
    DEFAULT_REMOTE_STORE_URI.to_string()
}

pub fn default_unknown() -> String {
    "unknown".to_string()
}

pub fn default_needed_system() -> String {
    "x86_64-linux".to_string()
}
