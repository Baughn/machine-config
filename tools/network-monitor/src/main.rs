use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::proto::rr::{Name, RecordType};
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use regex::Regex;
use serde::Deserialize;
use tokio::process::Command;
use tokio::time::{self, MissedTickBehavior};

#[derive(Parser, Debug)]
#[command(
    name = "network-monitor",
    version,
    about = "Minimal network health dashboard"
)]
struct Cli {
    /// Optional path to a configuration file in TOML format
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override the interface name from the configuration file
    #[arg(long)]
    interface: Option<String>,

    /// Override the refresh interval (milliseconds)
    #[arg(long)]
    interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default = "default_interval_ms")]
    interval_ms: u64,
    #[serde(default = "default_history_length")]
    history_length: usize,
    interface: Option<String>,
    #[serde(default)]
    targets: Vec<TargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TargetConfig {
    Icmp {
        name: String,
        address: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    Dns {
        name: String,
        query: String,
        resolver: Option<String>,
        #[serde(default = "default_record_type")]
        record_type: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
}

#[derive(Debug)]
struct MonitorTarget {
    name: String,
    history: VecDeque<Sample>,
    capacity: usize,
    kind: TargetKind,
}

#[derive(Debug)]
enum TargetKind {
    Icmp {
        address: String,
        timeout: Duration,
    },
    Dns {
        query: String,
        record_type: RecordType,
        resolver: ResolverKind,
        timeout: Duration,
    },
}

#[derive(Debug, Clone)]
enum ResolverKind {
    System,
    Custom(SocketAddr),
}

#[derive(Debug, Clone)]
struct Sample {
    success: bool,
    latency_ms: Option<f64>,
    message: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct NicState {
    operstate: Option<String>,
    carrier: Option<bool>,
    speed_mbps: Option<u32>,
    duplex: Option<String>,
    mtu: Option<u32>,
    rx_errors: Option<u64>,
    tx_errors: Option<u64>,
    rx_dropped: Option<u64>,
    tx_dropped: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut config = load_config(&cli)?;

    if let Some(interval) = cli.interval_ms {
        config.interval_ms = interval;
    }
    if let Some(interface) = cli.interface {
        config.interface = Some(interface);
    }

    let interface = match config.interface.clone() {
        Some(name) => name,
        None => detect_default_interface()
            .context("No interface specified and failed to determine a default")?,
    };

    if config.targets.is_empty() {
        return Err(anyhow!(
            "No targets configured. Provide a config file with at least one target."
        ));
    }

    let mut targets = prepare_targets(&config)?;
    let mut interval = time::interval(Duration::from_millis(config.interval_ms));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Consume the immediate first tick so we can probe right away.
    interval.tick().await;

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                println!("Exiting network-monitor");
                break;
            }
            _ = interval.tick() => {
                let nic_state = read_nic_state(&interface);
                for target in &mut targets {
                    if let Err(err) = target.probe().await {
                        eprintln!("Failed to probe {}: {err:?}", target.name);
                    }
                }
                if let Err(err) = render(&interface, &nic_state, &targets) {
                    eprintln!("Failed to render dashboard: {err:?}");
                }
            }
        }
    }

    Ok(())
}

fn load_config(cli: &Cli) -> Result<Config> {
    if let Some(path) = &cli.config {
        return read_config_file(path);
    }

    let default_config_path = default_config_path();

    if let Some(path) = default_config_path.as_ref() {
        if path.exists() {
            return read_config_file(path);
        }
    }

    let candidates = vec![
        PathBuf::from("./network-monitor.toml"),
        PathBuf::from("./config.toml"),
        PathBuf::from("./tools/network-monitor/config.toml"),
    ];

    for path in candidates {
        if path.exists() {
            return read_config_file(&path);
        }
    }

    if let Some(path) = default_config_path.as_ref() {
        print_missing_config_help(path);
        return Err(anyhow!(
            "No configuration file found. Save one to {} or pass --config <path>.",
            path.display()
        ));
    }

    Err(anyhow!(
        "No configuration file found. Set HOME or XDG_CONFIG_HOME, or pass --config <path>."
    ))
}

const EXAMPLE_CONFIG: &str = include_str!("../config.example.toml");

fn default_config_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(path)
                .join("network-monitor")
                .join("config.toml"),
        );
    }
    let home = env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("network-monitor")
            .join("config.toml"),
    )
}

fn print_missing_config_help(path: &Path) {
    println!("No configuration file found at {}", path.display());
    println!("Copy the example below to that location or supply --config <path>:\n");
    println!("{}", EXAMPLE_CONFIG.trim_end());
}

fn read_config_file(path: &Path) -> Result<Config> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;
    let config: Config = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file {}", path.display()))?;
    Ok(config)
}

fn prepare_targets(config: &Config) -> Result<Vec<MonitorTarget>> {
    config
        .targets
        .iter()
        .map(|cfg| MonitorTarget::from_config(cfg.clone(), config.history_length))
        .collect()
}

impl MonitorTarget {
    fn from_config(config: TargetConfig, capacity: usize) -> Result<Self> {
        let kind = match &config {
            TargetConfig::Icmp {
                address,
                timeout_ms,
                ..
            } => TargetKind::Icmp {
                address: address.clone(),
                timeout: Duration::from_millis(*timeout_ms),
            },
            TargetConfig::Dns {
                query,
                record_type,
                resolver,
                timeout_ms,
                ..
            } => {
                let record_type = parse_record_type(record_type)?;
                let resolver = match resolver {
                    Some(value) => ResolverKind::Custom(parse_resolver(value)?),
                    None => ResolverKind::System,
                };
                TargetKind::Dns {
                    query: query.clone(),
                    record_type,
                    resolver,
                    timeout: Duration::from_millis(*timeout_ms),
                }
            }
        };

        Ok(Self {
            name: match &config {
                TargetConfig::Icmp { name, .. } => name.clone(),
                TargetConfig::Dns { name, .. } => name.clone(),
            },
            history: VecDeque::with_capacity(capacity),
            capacity,
            kind,
        })
    }

    async fn probe(&mut self) -> Result<()> {
        let result = match &self.kind {
            TargetKind::Icmp { address, timeout } => probe_icmp(address, *timeout).await,
            TargetKind::Dns {
                query,
                record_type,
                resolver,
                timeout,
            } => probe_dns(query, *record_type, resolver.clone(), *timeout).await,
        };

        match result {
            Ok(sample) => self.push_sample(sample),
            Err(err) => {
                self.push_sample(Sample {
                    success: false,
                    latency_ms: None,
                    message: Some(err.to_string()),
                });
                return Err(err);
            }
        }

        Ok(())
    }

    fn push_sample(&mut self, sample: Sample) {
        if self.history.len() == self.capacity {
            self.history.pop_front();
        }
        self.history.push_back(sample);
    }
}

async fn probe_icmp(address: &str, timeout: Duration) -> Result<Sample> {
    let mut command = Command::new("ping");
    let timeout_secs = std::cmp::max(1, (timeout.as_millis() as u64 + 999) / 1000);
    command.arg("-n");
    command.arg("-c").arg("1");
    command.arg("-w").arg(timeout_secs.to_string());
    command.arg(address);

    let start = Instant::now();
    let output = command.output().await.context("Failed to spawn ping")?;
    let elapsed = start.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let latency = parse_ping_latency(&stdout);
    let success = output.status.success();

    let message = if success {
        None
    } else if !stderr.trim().is_empty() {
        Some(stderr.trim().to_string())
    } else {
        Some(stdout.lines().last().unwrap_or("ping failed").to_string())
    };

    Ok(Sample {
        success,
        latency_ms: if success {
            latency.or_else(|| Some(duration_to_ms(elapsed)))
        } else {
            None
        },
        message,
    })
}

async fn probe_dns(
    query: &str,
    record_type: RecordType,
    resolver: ResolverKind,
    timeout: Duration,
) -> Result<Sample> {
    let attempt = async move {
        let (resolver, description) = match resolver {
            ResolverKind::System => {
                let resolver = TokioAsyncResolver::tokio_from_system_conf()
                    .context("Failed to initialise system resolver")?;
                (resolver, String::from("system"))
            }
            ResolverKind::Custom(addr) => {
                let mut config = ResolverConfig::new();
                config.add_name_server(NameServerConfig::new(addr, Protocol::Udp));
                let resolver = TokioAsyncResolver::tokio(config, ResolverOpts::default());
                (resolver, addr.to_string())
            }
        };

        let name = Name::from_ascii(query).context("Invalid DNS name")?;
        let start = Instant::now();
        let lookup = resolver
            .lookup(name, record_type)
            .await
            .with_context(|| format!("Lookup failed via {}", description))?;
        let latency_ms = duration_to_ms(start.elapsed());

        Ok::<Sample, anyhow::Error>(Sample {
            success: true,
            latency_ms: Some(latency_ms),
            message: Some(format!(
                "{:?} records: {}",
                record_type,
                lookup.iter().count()
            )),
        })
    };

    match tokio::time::timeout(timeout, attempt).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!("DNS lookup timed out")),
    }
}

fn render(interface: &str, nic: &NicState, targets: &[MonitorTarget]) -> io::Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "\x1b[2J\x1b[H")?;

    writeln!(stdout, "Network Monitor")?;
    writeln!(stdout, "Interface: {}", interface)?;

    let nic_summary = format_nic_summary(nic);
    writeln!(stdout, "{}", nic_summary)?;
    writeln!(stdout)?;

    writeln!(stdout, "Targets:")?;
    for target in targets {
        writeln!(stdout, "{}", format_target_line(target))?;
    }

    stdout.flush()
}

fn format_target_line(target: &MonitorTarget) -> String {
    let (percent, bar) = compute_availability(&target.history, target.capacity);
    let recent_latency = target
        .history
        .iter()
        .rev()
        .find(|sample| sample.success && sample.latency_ms.is_some())
        .and_then(|sample| sample.latency_ms)
        .map(|ms| format!("{ms:.1} ms"))
        .unwrap_or_else(|| "--".to_string());

    let colorized_percent = colorize_percent(percent);
    let colorized_bar = colorize_bar(&bar, percent);

    let last_message = target
        .history
        .back()
        .and_then(|sample| sample.message.clone())
        .unwrap_or_default();

    format!(
        "{:<18} {:>8} {}   last latency: {:>8}   {}",
        target.name, colorized_percent, colorized_bar, recent_latency, last_message
    )
}

fn compute_availability(history: &VecDeque<Sample>, capacity: usize) -> (f64, String) {
    if capacity == 0 {
        return (100.0, String::new());
    }

    let successes = history.iter().filter(|sample| sample.success).count();
    let total = history.len();

    let percent = if total == 0 {
        0.0
    } else {
        (successes as f64 / total as f64) * 100.0
    };

    let mut bar = String::with_capacity(capacity);
    let missing = capacity.saturating_sub(history.len());
    for _ in 0..missing {
        bar.push('·');
    }
    for sample in history {
        bar.push(if sample.success { '█' } else { '░' });
    }

    (percent, bar)
}

fn colorize_percent(percent: f64) -> String {
    let text = format!("{percent:5.1}%");
    if approx_equal(percent, 100.0) {
        text.bright_green().to_string()
    } else if percent >= 90.0 {
        text.yellow().to_string()
    } else {
        text.red().to_string()
    }
}

fn colorize_bar(bar: &str, percent: f64) -> String {
    if bar.is_empty() {
        return bar.to_string();
    }

    if approx_equal(percent, 100.0) {
        bar.bright_green().to_string()
    } else if percent >= 90.0 {
        bar.yellow().to_string()
    } else {
        bar.red().to_string()
    }
}

fn format_nic_summary(nic: &NicState) -> String {
    let mut parts = Vec::new();
    if let Some(state) = &nic.operstate {
        parts.push(format!("state={state}"));
    }
    if let Some(carrier) = nic.carrier {
        let symbol = if carrier { '●' } else { '○' };
        let coloured = if carrier {
            symbol.bright_green().to_string()
        } else {
            symbol.red().to_string()
        };
        parts.push(format!("link={coloured}"));
    }
    if let Some(speed) = nic.speed_mbps {
        parts.push(format!("speed={} Mb/s", speed));
    }
    if let Some(duplex) = &nic.duplex {
        parts.push(format!("duplex={duplex}"));
    }
    if let Some(mtu) = nic.mtu {
        parts.push(format!("mtu={mtu}"));
    }
    if let Some(rx) = nic.rx_errors {
        parts.push(format!("rx_err={rx}"));
    }
    if let Some(tx) = nic.tx_errors {
        parts.push(format!("tx_err={tx}"));
    }
    if let Some(rx) = nic.rx_dropped {
        parts.push(format!("rx_drop={rx}"));
    }
    if let Some(tx) = nic.tx_dropped {
        parts.push(format!("tx_drop={tx}"));
    }

    if parts.is_empty() {
        "No NIC statistics available".to_string()
    } else {
        parts.join(", ")
    }
}

fn read_nic_state(interface: &str) -> NicState {
    let base = Path::new("/sys/class/net").join(interface);
    let mut nic = NicState::default();

    nic.operstate = read_trimmed(base.join("operstate"));
    nic.carrier = read_trimmed(base.join("carrier")).and_then(|s| match s.as_str() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    });
    nic.speed_mbps = read_trimmed(base.join("speed"))
        .and_then(|s| s.parse::<i64>().ok())
        .and_then(|v| if v >= 0 { Some(v as u32) } else { None });
    nic.duplex = read_trimmed(base.join("duplex"));
    nic.mtu = read_trimmed(base.join("mtu")).and_then(|s| s.parse().ok());
    nic.rx_errors = read_trimmed(base.join("statistics/rx_errors")).and_then(|s| s.parse().ok());
    nic.tx_errors = read_trimmed(base.join("statistics/tx_errors")).and_then(|s| s.parse().ok());
    nic.rx_dropped = read_trimmed(base.join("statistics/rx_dropped")).and_then(|s| s.parse().ok());
    nic.tx_dropped = read_trimmed(base.join("statistics/tx_dropped")).and_then(|s| s.parse().ok());

    nic
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_record_type(value: &str) -> Result<RecordType> {
    match value.to_uppercase().as_str() {
        "A" => Ok(RecordType::A),
        "AAAA" => Ok(RecordType::AAAA),
        "MX" => Ok(RecordType::MX),
        "TXT" => Ok(RecordType::TXT),
        "NS" => Ok(RecordType::NS),
        "CAA" => Ok(RecordType::CAA),
        "SRV" => Ok(RecordType::SRV),
        other => Err(anyhow!("Unsupported record type: {other}")),
    }
}

fn parse_resolver(value: &str) -> Result<SocketAddr> {
    if let Ok(addr) = value.parse() {
        return Ok(addr);
    }

    if let Ok(ip) = value.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, 53));
    }

    Err(anyhow!("Invalid resolver address: {value}"))
}

fn parse_ping_latency(output: &str) -> Option<f64> {
    static LATENCY_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"time=([0-9]*\.?[0-9]+)\s*ms").expect("valid latency regex"));

    LATENCY_RE
        .captures_iter(output)
        .last()
        .and_then(|caps| caps.get(1))
        .and_then(|value| value.as_str().parse::<f64>().ok())
}

fn duration_to_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn default_interval_ms() -> u64 {
    2000
}

fn default_history_length() -> usize {
    30
}

fn default_timeout_ms() -> u64 {
    1500
}

fn default_record_type() -> String {
    "A".to_string()
}

fn approx_equal(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.0001
}

fn detect_default_interface() -> Result<String> {
    let entries = fs::read_dir("/sys/class/net")?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "lo" {
            continue;
        }
        let state = read_trimmed(entry.path().join("operstate"));
        let carrier = read_trimmed(entry.path().join("carrier"));
        if matches!(state.as_deref(), Some("up") | Some("unknown"))
            && matches!(carrier.as_deref(), Some("1") | None)
        {
            return Ok(name.into_owned());
        }
    }

    Err(anyhow!("Unable to detect an active interface"))
}
