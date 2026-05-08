use clap::{Args, Parser, Subcommand, ValueEnum};
use procfs::{Current, CurrentSI, KernelStats, Meminfo, MemoryPressure};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_UNKNOWN_P95_MS: u64 = 30 * 60 * 1000;
const DEFAULT_STALE_START_MS: u128 = 24 * 60 * 60 * 1000;
const DEFAULT_MAX_SAMPLES_PER_PNAME: usize = 200;
const DEFAULT_STALE_TELEMETRY_MS: u128 = 10_000;
const DEFAULT_MAX_REMOTE_ADMITTED: usize = 16;
const DEFAULT_MAX_UNKNOWN_REMOTE: usize = 1;
const DEFAULT_MIN_REMOTE_ADMISSION_INTERVAL_MS: u128 = 1_000;
const DEFAULT_EXPLORATION_PERCENT: u64 = 20;
const DEFAULT_EXPLORATION_MIN_SAMPLES: u64 = 4;
const DEFAULT_REMOTE_HOST: &str = "tsugumi";
const DEFAULT_REMOTE_STORE_URI: &str = "ssh-ng://svein@tsugumi.local";
const DEFAULT_REMOTE_BUILDER: &str =
    "ssh-ng://svein@tsugumi.local x86_64-linux /home/svein/.ssh/id_ed25519 16 1 nixos-test,kvm,big-parallel - -";
const DEFAULT_LOCAL_CAPACITY: usize = 32;
const DEFAULT_REMOTE_CAPACITY: usize = 16;
const SCHEMA_VERSION: i64 = 1;
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

#[derive(Parser)]
#[command(name = "nix-build-balancer", arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand)]
enum CliCommand {
    Serve(ServeArgs),
    Event(EventArgs),
    Hook(HookArgs),
    Telemetry(ServeArgs),
}

#[derive(Args)]
struct ServeArgs {
    #[arg(long, value_enum, default_value = "agent")]
    mode: Mode,
    #[arg(long, default_value_t = hostname_fallback())]
    host: String,
    #[arg(long, default_value = "/var/lib/nix-build-balancer")]
    data_dir: PathBuf,
    #[arg(long)]
    unix_socket: Option<PathBuf>,
    #[arg(long)]
    listen: Option<String>,
    #[arg(long)]
    remote: Vec<String>,
    #[arg(long, default_value_t = 1_000)]
    poll_interval_ms: u64,
    #[arg(long, default_value_t = DEFAULT_MAX_SAMPLES_PER_PNAME)]
    max_samples_per_pname: usize,
    #[arg(long, default_value_t = DEFAULT_STALE_START_MS)]
    stale_start_ms: u128,
    #[arg(long)]
    once: bool,
}

#[derive(Args)]
struct EventArgs {
    #[arg(long)]
    endpoint: String,
    #[arg(long)]
    kind: String,
    #[arg(long)]
    drv_path: String,
    #[arg(long, default_value = "")]
    out_paths: String,
    #[arg(long, default_value = "unknown")]
    status: String,
    #[arg(long, default_value_t = hostname_fallback())]
    host: String,
}

#[derive(Args)]
struct HookArgs {
    #[arg(long)]
    endpoint: String,
    #[arg(long, default_value_t = hostname_fallback())]
    host: String,
    #[arg(long, default_value = DEFAULT_REMOTE_HOST)]
    remote_host: String,
    #[arg(long, default_value = DEFAULT_REMOTE_STORE_URI)]
    remote_store_uri: String,
    #[arg(long, default_value = DEFAULT_REMOTE_BUILDER)]
    remote_builder: String,
    #[arg(long, default_value = "nix")]
    nix_bin: String,
    #[arg(value_name = "VERBOSITY", allow_hyphen_values = true, trailing_var_arg = true, num_args = 1..)]
    rest: Vec<String>,
}

#[derive(Clone, Debug)]
struct Config {
    mode: Mode,
    host: String,
    data_dir: PathBuf,
    unix_socket: Option<PathBuf>,
    listen: Option<String>,
    remote: Vec<String>,
    poll_interval: Duration,
    max_samples_per_pname: usize,
    stale_start_ms: u128,
    once: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Agent,
    Controller,
}

#[derive(Debug, Serialize, Deserialize)]
struct Telemetry {
    #[serde(default)]
    host: String,
    #[serde(default)]
    timestamp_ms: u128,
    cpu_busy_ratio: Option<f64>,
    mem_total_kb: Option<u64>,
    mem_available_kb: Option<u64>,
    psi_memory_some_avg10: Option<f64>,
    #[serde(default)]
    nix_slots_total: usize,
    #[serde(default)]
    nix_slots_local: usize,
    #[serde(default)]
    nix_slots_remote: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct BuildEvent {
    #[serde(default = "default_unknown")]
    kind: String,
    #[serde(default)]
    drv_path: String,
    #[serde(default)]
    out_paths: String,
    #[serde(default = "default_unknown")]
    status: String,
    #[serde(default = "hostname_fallback")]
    host: String,
    #[serde(default = "now_ms")]
    timestamp_ms: u128,
}

#[derive(Clone, Debug)]
struct HookConfig {
    endpoint: String,
    host: String,
    remote_host: String,
    remote_store_uri: String,
    remote_builder: String,
    nix_bin: String,
    verbosity: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BuildCandidate {
    #[serde(default)]
    am_willing: u64,
    #[serde(default = "default_needed_system")]
    needed_system: String,
    drv_path: String,
    #[serde(default)]
    required_features: Vec<String>,
    #[serde(default)]
    pname: String,
    #[serde(default = "default_remote_host")]
    remote_host: String,
    #[serde(default = "default_remote_store_uri")]
    remote_store_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Decision {
    decision: String,
    reason: String,
    store_uri: Option<String>,
    #[serde(skip_serializing, default)]
    metrics: Option<DecisionMetrics>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DecisionMetrics {
    local_samples: u64,
    remote_samples: u64,
    local_prediction_ms: u64,
    remote_prediction_ms: u64,
    local_queue_ms: u64,
    remote_queue_ms: u64,
    local_completion_ms: u64,
    remote_completion_ms: u64,
    local_slots: usize,
    remote_slots: usize,
    local_active_count: usize,
    admitted_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct PackageStats {
    count: u64,
    p95_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct StatsResponse {
    unknown_p95_ms: u64,
    packages: Vec<PackageStatsEntry>,
}

#[derive(Serialize, Deserialize)]
struct PackageStatsEntry {
    pname: String,
    count: u64,
    p50_ms: u64,
    p80_ms: u64,
    p95_ms: u64,
}

#[derive(Debug)]
struct SchedulerConfig {
    local_host_name: String,
    remote_target: BuildTarget,
    policy: SchedulerPolicy,
}

#[derive(Clone, Debug)]
struct SchedulerPolicy {
    unknown_p95_ms: u64,
    stale_telemetry_ms: u128,
    max_remote_admitted: usize,
    max_unknown_remote: usize,
    min_remote_admission_interval_ms: u128,
    exploration_percent: u64,
    exploration_min_samples: u64,
    local_capacity: usize,
    remote_capacity: usize,
    max_remote_cpu_busy_ratio: f64,
    max_remote_memory_pressure_avg10: f64,
    min_remote_mem_available_kb: u64,
}

#[derive(Debug)]
struct BuildTarget {
    host_name: String,
    store_uri: String,
    capacity: usize,
}

#[derive(Debug)]
struct HostState {
    telemetry: Telemetry,
    stats: Option<PackageStats>,
    active_count: usize,
    active_queue_ms: u64,
    admissions: Vec<Admission>,
}

#[derive(Debug)]
struct Prediction {
    samples: u64,
    package_ms: u64,
    queue_ms: u64,
    completion_ms: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum Eligibility {
    Accepted,
    Declined { reason: &'static str },
}

#[derive(Debug)]
struct DecisionOutcome {
    decision: Decision,
    record_remote_admission: bool,
}

impl SchedulerConfig {
    fn from_candidate(cfg: &Config, candidate: &BuildCandidate) -> Self {
        let policy = SchedulerPolicy::default();
        Self {
            local_host_name: cfg.host.clone(),
            remote_target: BuildTarget::from_candidate(candidate, &policy),
            policy,
        }
    }
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self {
            unknown_p95_ms: DEFAULT_UNKNOWN_P95_MS,
            stale_telemetry_ms: DEFAULT_STALE_TELEMETRY_MS,
            max_remote_admitted: DEFAULT_MAX_REMOTE_ADMITTED,
            max_unknown_remote: DEFAULT_MAX_UNKNOWN_REMOTE,
            min_remote_admission_interval_ms: DEFAULT_MIN_REMOTE_ADMISSION_INTERVAL_MS,
            exploration_percent: DEFAULT_EXPLORATION_PERCENT,
            exploration_min_samples: DEFAULT_EXPLORATION_MIN_SAMPLES,
            local_capacity: DEFAULT_LOCAL_CAPACITY,
            remote_capacity: DEFAULT_REMOTE_CAPACITY,
            max_remote_cpu_busy_ratio: 0.90,
            max_remote_memory_pressure_avg10: 10.0,
            min_remote_mem_available_kb: 4 * 1024 * 1024,
        }
    }
}

impl BuildTarget {
    fn from_candidate(candidate: &BuildCandidate, policy: &SchedulerPolicy) -> Self {
        Self {
            host_name: candidate.remote_host.clone(),
            store_uri: candidate.remote_store_uri.clone(),
            capacity: policy.remote_capacity,
        }
    }
}

fn main() {
    if let Err(err) = real_main() {
        eprintln!("nix-build-balancer: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> io::Result<()> {
    match Cli::parse().command {
        Some(CliCommand::Serve(args)) => serve(serve_config(args, true)?),
        Some(CliCommand::Event(args)) => send_event(args.into()),
        Some(CliCommand::Hook(args)) => run_hook(args.try_into()?),
        Some(CliCommand::Telemetry(args)) => {
            let cfg = serve_config(args, false)?;
            println!("{}", telemetry_json(&read_telemetry(&cfg.host)?));
            Ok(())
        }
        None => Ok(()),
    }
}

fn serve_config(args: ServeArgs, require_listener: bool) -> io::Result<Config> {
    let cfg = Config {
        mode: args.mode,
        host: args.host,
        data_dir: args.data_dir,
        unix_socket: args.unix_socket,
        listen: args.listen,
        remote: args.remote,
        poll_interval: Duration::from_millis(args.poll_interval_ms),
        max_samples_per_pname: args.max_samples_per_pname,
        stale_start_ms: args.stale_start_ms,
        once: args.once,
    };

    if require_listener && cfg.unix_socket.is_none() && cfg.listen.is_none() && !cfg.once {
        return invalid("serve needs --unix-socket or --listen");
    }

    Ok(cfg)
}

impl From<EventArgs> for (String, BuildEvent) {
    fn from(args: EventArgs) -> Self {
        (
            args.endpoint,
            BuildEvent {
                kind: args.kind,
                drv_path: args.drv_path,
                out_paths: args.out_paths,
                status: args.status,
                host: args.host,
                timestamp_ms: now_ms(),
            },
        )
    }
}

impl TryFrom<HookArgs> for HookConfig {
    type Error = io::Error;

    fn try_from(args: HookArgs) -> io::Result<Self> {
        let Some(verbosity) = args.rest.first().cloned() else {
            return invalid("verbosity is required");
        };
        Ok(Self {
            endpoint: args.endpoint,
            host: args.host,
            remote_host: args.remote_host,
            remote_store_uri: args.remote_store_uri,
            remote_builder: args.remote_builder,
            nix_bin: args.nix_bin,
            verbosity,
        })
    }
}

fn invalid<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn default_unknown() -> String {
    "unknown".to_string()
}

fn default_needed_system() -> String {
    "x86_64-linux".to_string()
}

fn default_remote_host() -> String {
    DEFAULT_REMOTE_HOST.to_string()
}

fn default_remote_store_uri() -> String {
    DEFAULT_REMOTE_STORE_URI.to_string()
}

fn serve(cfg: Config) -> io::Result<()> {
    fs::create_dir_all(&cfg.data_dir)?;
    cleanup_state(&cfg)?;

    if cfg.once {
        println!("{}", telemetry_json(&read_telemetry(&cfg.host)?));
        return Ok(());
    }

    if matches!(cfg.mode, Mode::Controller) && !cfg.remote.is_empty() {
        let poll_cfg = cfg.clone();
        thread::spawn(move || poll_remotes(poll_cfg));
    }

    if let Some(path) = cfg.unix_socket.clone() {
        let unix_cfg = cfg.clone();
        thread::spawn(move || {
            if let Err(err) = serve_unix(path, unix_cfg) {
                eprintln!("unix listener stopped: {err}");
            }
        });
    }

    if let Some(addr) = cfg.listen.clone() {
        serve_tcp(&addr, cfg)?;
    } else {
        loop {
            thread::park();
        }
    }
    Ok(())
}

fn serve_unix(path: PathBuf, cfg: Config) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(path)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cfg = cfg.clone();
                thread::spawn(move || {
                    let _ = handle_stream(stream, &cfg);
                });
            }
            Err(err) => eprintln!("unix accept failed: {err}"),
        }
    }
    Ok(())
}

fn serve_tcp(addr: &str, cfg: Config) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cfg = cfg.clone();
                thread::spawn(move || {
                    let _ = handle_stream(stream, &cfg);
                });
            }
            Err(err) => eprintln!("tcp accept failed: {err}"),
        }
    }
    Ok(())
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

fn handle_stream<S: ReadWrite>(mut stream: S, cfg: &Config) -> io::Result<()> {
    let mut request = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        request.extend_from_slice(&buf[..n]);
        if request.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if request.len() > 64 * 1024 {
            return write_response(&mut stream, 413, "text/plain", "request too large\n");
        }
    }

    let header_end = request
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| pos + 4)
        .unwrap_or(request.len());
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);

    let mut body = request[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    body.truncate(content_length);

    match (method, path) {
        ("GET", "/health") => {
            write_response(&mut stream, 200, "application/json", "{\"ok\":true}\n")
        }
        ("GET", "/telemetry") => {
            let telemetry = read_telemetry(&cfg.host)?;
            write_response(
                &mut stream,
                200,
                "application/json",
                &telemetry_json(&telemetry),
            )
        }
        ("GET", "/stats") => {
            let stats = stats_json(&cfg.data_dir)?;
            write_response(&mut stream, 200, "application/json", &stats)
        }
        ("POST", "/event/build-start") | ("POST", "/event/build-finish") => {
            let mut event = parse_event_body(&body)?;
            event.kind = if path.ends_with("build-start") {
                "start".to_string()
            } else {
                "finish".to_string()
            };
            record_event(cfg, &event)?;
            write_response(&mut stream, 200, "application/json", "{\"ok\":true}\n")
        }
        ("POST", "/event/admission-finish") => {
            let event = parse_event_body(&body)?;
            finish_admission(&cfg.data_dir, &event.drv_path)?;
            write_response(&mut stream, 200, "application/json", "{\"ok\":true}\n")
        }
        ("POST", "/decision/build-candidate") => {
            let candidate = parse_candidate_body(&body)?;
            let decision = decide_build_candidate(cfg, &candidate)?;
            log_scheduler_decision(cfg, &candidate, &decision);
            write_response(
                &mut stream,
                200,
                "application/json",
                &decision_json(&decision),
            )
        }
        _ => write_response(&mut stream, 404, "text/plain", "not found\n"),
    }
}

fn write_response<W: Write>(
    stream: &mut W,
    status: u16,
    content_type: &str,
    body: &str,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn send_event((endpoint, event): (String, BuildEvent)) -> io::Result<()> {
    let path = if event.kind == "start" {
        "/event/build-start"
    } else {
        "/event/build-finish"
    };
    let body = event_body(&event);
    let request = format!(
        "POST {path} HTTP/1.1\r\nhost: nix-build-balancer\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );

    if let Some(path) = endpoint.strip_prefix("unix:") {
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(request.as_bytes())?;
        drain_response(stream)
    } else if let Some(addr) = endpoint.strip_prefix("tcp:") {
        let mut stream = TcpStream::connect(addr)?;
        stream.write_all(request.as_bytes())?;
        drain_response(stream)
    } else {
        invalid("endpoint must start with unix: or tcp:")
    }
}

/// Run the Nix build hook loop.
///
/// Nix can offer multiple `try` candidates to a build hook. This loop declines
/// candidates until the local daemon accepts one, then delegates exactly that
/// candidate to `nix __build-remote`.
fn run_hook(cfg: HookConfig) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let settings = read_hook_settings(&mut stdin)?;

    loop {
        let Some(candidate) = read_hook_candidate(&mut stdin)? else {
            return Ok(());
        };

        let decision = request_decision(&cfg, &candidate).unwrap_or_else(|err| Decision {
            decision: "decline".to_string(),
            reason: format!("daemon unavailable: {err}"),
            store_uri: None,
            metrics: None,
        });

        if decision.decision != "accept" {
            eprintln!("# decline");
            continue;
        }

        return delegate_remote_build(&cfg, &settings, &candidate, &mut stdin);
    }
}

/// Read the initial key/value setting stream from Nix's build-hook protocol.
///
/// The stream is terminated by a zero marker. Each preceding entry contains a
/// setting name and value encoded with Nix's padded string format.
fn read_hook_settings<R: Read>(reader: &mut R) -> io::Result<Vec<(String, String)>> {
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
///
/// Returns `Ok(None)` when Nix closes the stream or sends an operation this hook
/// does not handle. Supported candidates are `try` records.
fn read_hook_candidate<R: Read>(reader: &mut R) -> io::Result<Option<BuildCandidate>> {
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

/// Delegate an accepted candidate to the stock Nix remote build helper.
///
/// The child process receives the original hook settings plus a replacement
/// `builders` setting containing only the selected builder line. This keeps Nix
/// from choosing a different remote host than the scheduler admitted.
fn delegate_remote_build<R: Read>(
    cfg: &HookConfig,
    settings: &[(String, String)],
    candidate: &BuildCandidate,
    parent_stdin: &mut R,
) -> io::Result<()> {
    let mut child = Command::new(&cfg.nix_bin)
        .arg("__build-remote")
        .arg(&cfg.verbosity)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::inherit())
        .spawn()?;

    {
        let child_stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing child stdin"))?;
        write_hook_settings(child_stdin, settings, &cfg.remote_builder)?;
        write_hook_candidate(child_stdin, candidate)?;
        child_stdin.flush()?;
    }

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "missing child stderr"))?;
    let accepted = proxy_child_until_decision(stderr)?;
    if !accepted {
        let _ = child.stdin.take();
        let _ = child.wait();
        let finish = BuildEvent {
            kind: "admission-finish".to_string(),
            drv_path: candidate.drv_path.clone(),
            out_paths: String::new(),
            status: "cancelled".to_string(),
            host: cfg.host.clone(),
            timestamp_ms: now_ms(),
        };
        let _ = post_endpoint(
            &cfg.endpoint,
            "/event/admission-finish",
            &event_body(&finish),
        );
        return Ok(());
    }

    let inputs = read_nix_strings(parent_stdin)?;
    let wanted_outputs = read_nix_strings(parent_stdin)?;
    if let Some(child_stdin) = child.stdin.as_mut() {
        write_nix_strings(child_stdin, &inputs)?;
        write_nix_strings(child_stdin, &wanted_outputs)?;
        child_stdin.flush()?;
    }
    let _ = child.stdin.take();

    let status = child.wait()?;
    let finish = BuildEvent {
        kind: "admission-finish".to_string(),
        drv_path: candidate.drv_path.clone(),
        out_paths: String::new(),
        status: if status.success() {
            "success".to_string()
        } else {
            "failure".to_string()
        },
        host: cfg.host.clone(),
        timestamp_ms: now_ms(),
    };
    let _ = post_endpoint(
        &cfg.endpoint,
        "/event/admission-finish",
        &event_body(&finish),
    );

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "delegated nix __build-remote exited with {status}"
        )))
    }
}

/// Proxy child stderr until `nix __build-remote` accepts or declines the build.
///
/// Nix communicates hook decisions on stderr. Once the child accepts, the rest
/// of stderr is copied in a background thread so diagnostics remain visible
/// while the parent continues forwarding binary protocol data.
fn proxy_child_until_decision(stderr: ChildStderr) -> io::Result<bool> {
    let mut reader = BufReader::new(stderr);
    let mut accepted = false;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(false);
        }
        eprint!("{line}");
        let trimmed = line.trim_end();
        if trimmed == "# accept" {
            let mut store_uri = String::new();
            let n = reader.read_line(&mut store_uri)?;
            if n == 0 {
                return Ok(false);
            }
            eprint!("{store_uri}");
            accepted = true;
            break;
        }
        if trimmed == "# decline" || trimmed == "# decline-permanently" || trimmed == "# postpone" {
            break;
        }
    }

    if accepted {
        thread::spawn(move || {
            let mut reader = reader;
            let mut stderr = io::stderr();
            let _ = io::copy(&mut reader, &mut stderr);
        });
    }
    Ok(accepted)
}

/// Write hook settings to a delegated `nix __build-remote` child.
///
/// Existing settings are preserved, then `builders` is appended with the single
/// builder line selected by this balancer. Nix uses the later setting when
/// evaluating the delegated remote build.
fn write_hook_settings<W: Write>(
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
fn write_hook_candidate<W: Write>(writer: &mut W, candidate: &BuildCandidate) -> io::Result<()> {
    write_nix_string(writer, "try")?;
    write_nix_u64(writer, candidate.am_willing)?;
    write_nix_string(writer, &candidate.needed_system)?;
    write_nix_string(writer, &candidate.drv_path)?;
    write_nix_strings(writer, &candidate.required_features)
}

/// Read a little-endian 64-bit integer from the Nix hook protocol.
fn read_nix_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Write a little-endian 64-bit integer to the Nix hook protocol.
fn write_nix_u64<W: Write>(writer: &mut W, value: u64) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

/// Read a Nix protocol string and consume its 8-byte alignment padding.
fn read_nix_string<R: Read>(reader: &mut R) -> io::Result<String> {
    let len = read_nix_u64(reader)? as usize;
    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes)?;
    read_nix_padding(reader, len)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Write a Nix protocol string with 8-byte alignment padding.
fn write_nix_string<W: Write>(writer: &mut W, value: &str) -> io::Result<()> {
    let bytes = value.as_bytes();
    write_nix_u64(writer, bytes.len() as u64)?;
    writer.write_all(bytes)?;
    write_nix_padding(writer, bytes.len())
}

/// Read a counted list of Nix protocol strings.
fn read_nix_strings<R: Read>(reader: &mut R) -> io::Result<Vec<String>> {
    let count = read_nix_u64(reader)?;
    let mut values = Vec::new();
    for _ in 0..count {
        values.push(read_nix_string(reader)?);
    }
    Ok(values)
}

/// Write a counted list of Nix protocol strings.
fn write_nix_strings<W: Write>(writer: &mut W, values: &[String]) -> io::Result<()> {
    write_nix_u64(writer, values.len() as u64)?;
    for value in values {
        write_nix_string(writer, value)?;
    }
    Ok(())
}

/// Consume and validate the zero padding after a Nix protocol string.
fn read_nix_padding<R: Read>(reader: &mut R, len: usize) -> io::Result<()> {
    let padding = padding_len(len);
    if padding > 0 {
        let mut buf = vec![0u8; padding];
        reader.read_exact(&mut buf)?;
        if buf.iter().any(|byte| *byte != 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "non-zero nix protocol padding",
            ));
        }
    }
    Ok(())
}

/// Write zero padding after a Nix protocol string.
fn write_nix_padding<W: Write>(writer: &mut W, len: usize) -> io::Result<()> {
    let padding = padding_len(len);
    if padding > 0 {
        writer.write_all(&vec![0u8; padding])?;
    }
    Ok(())
}

/// Return the number of padding bytes needed for Nix's 8-byte string alignment.
fn padding_len(len: usize) -> usize {
    if len.is_multiple_of(8) {
        0
    } else {
        8 - (len % 8)
    }
}

fn request_decision(cfg: &HookConfig, candidate: &BuildCandidate) -> io::Result<Decision> {
    let body = candidate_json(candidate, cfg);
    let response = post_endpoint(&cfg.endpoint, "/decision/build-candidate", &body)?;
    serde_json::from_str(&response).map_err(json_error)
}

fn drain_response<R: Read>(mut stream: R) -> io::Result<()> {
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.starts_with("HTTP/1.1 200 ") {
        Ok(())
    } else {
        Err(io::Error::other(response))
    }
}

fn post_endpoint(endpoint: &str, path: &str, body: &str) -> io::Result<String> {
    let request = format!(
        "POST {path} HTTP/1.1\r\nhost: nix-build-balancer\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );

    if let Some(path) = endpoint.strip_prefix("unix:") {
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(request.as_bytes())?;
        read_http_body(stream)
    } else if let Some(addr) = endpoint.strip_prefix("tcp:") {
        let mut stream = TcpStream::connect(addr)?;
        stream.write_all(request.as_bytes())?;
        read_http_body(stream)
    } else {
        invalid("endpoint must start with unix: or tcp:")
    }
}

fn read_http_body<R: Read>(mut stream: R) -> io::Result<String> {
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return invalid("invalid HTTP response");
    };
    if headers.starts_with("HTTP/1.1 200 ") {
        Ok(body.to_string())
    } else {
        Err(io::Error::other(response))
    }
}

fn event_body(event: &BuildEvent) -> String {
    json_line(event)
}

fn parse_event_body(body: &[u8]) -> io::Result<BuildEvent> {
    serde_json::from_slice(body).map_err(json_error)
}

fn parse_candidate_body(body: &[u8]) -> io::Result<BuildCandidate> {
    let mut candidate: BuildCandidate = serde_json::from_slice(body).map_err(json_error)?;
    if candidate.am_willing == 0 {
        candidate.am_willing = 1;
    }
    if candidate.pname.is_empty() {
        candidate.pname = pname_from_drv(&candidate.drv_path);
    }
    Ok(candidate)
}

fn log_scheduler_decision(cfg: &Config, candidate: &BuildCandidate, decision: &Decision) {
    eprintln!("{}", scheduler_decision_log_line(cfg, candidate, decision));
}

fn scheduler_decision_log_line(
    cfg: &Config,
    candidate: &BuildCandidate,
    decision: &Decision,
) -> String {
    let required_features = candidate.required_features.join(",");
    let mut line = format!(
        "scheduler_decision host={} remote_host={} decision={} reason={} pname={} drv_path={} needed_system={} required_features={} store_uri={}",
        quoted_log_value(&cfg.host),
        quoted_log_value(&candidate.remote_host),
        quoted_log_value(&decision.decision),
        quoted_log_value(&decision.reason),
        quoted_log_value(&candidate.pname),
        quoted_log_value(&candidate.drv_path),
        quoted_log_value(&candidate.needed_system),
        quoted_log_value(&required_features),
        decision
            .store_uri
            .as_ref()
            .map(|value| quoted_log_value(value))
            .unwrap_or_else(|| "null".to_string())
    );
    if let Some(metrics) = &decision.metrics {
        line.push_str(&format!(
            " local_samples={} remote_samples={} local_prediction_ms={} remote_prediction_ms={} local_queue_ms={} remote_queue_ms={} local_completion_ms={} remote_completion_ms={} local_slots={} remote_slots={} local_active_count={} admitted_count={}",
            metrics.local_samples,
            metrics.remote_samples,
            metrics.local_prediction_ms,
            metrics.remote_prediction_ms,
            metrics.local_queue_ms,
            metrics.remote_queue_ms,
            metrics.local_completion_ms,
            metrics.remote_completion_ms,
            metrics.local_slots,
            metrics.remote_slots,
            metrics.local_active_count,
            metrics.admitted_count
        ));
    }
    line
}

fn quoted_log_value(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn candidate_json(candidate: &BuildCandidate, cfg: &HookConfig) -> String {
    let body = BuildCandidate {
        am_willing: candidate.am_willing,
        needed_system: candidate.needed_system.clone(),
        drv_path: candidate.drv_path.clone(),
        required_features: candidate.required_features.clone(),
        pname: candidate.pname.clone(),
        remote_host: cfg.remote_host.clone(),
        remote_store_uri: cfg.remote_store_uri.clone(),
    };
    json_line(&body)
}

fn decision_json(decision: &Decision) -> String {
    json_line(decision)
}

fn json_line<T: Serialize>(value: &T) -> String {
    let mut out = serde_json::to_string(value).unwrap_or_else(|err| {
        format!(
            "{{\"error\":{}}}",
            serde_json::to_string(&err.to_string())
                .unwrap_or_else(|_| "\"serialization failed\"".to_string())
        )
    });
    out.push('\n');
    out
}

fn json_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

fn record_event(cfg: &Config, event: &BuildEvent) -> io::Result<()> {
    let mut conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_starts(&conn, cfg.stale_start_ms)?;

    let pname = pname_from_drv(&event.drv_path);
    if event.kind == "start" {
        conn.execute(
            "INSERT INTO active_builds (drv_path, host, pname, started_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(drv_path) DO UPDATE SET
               host = excluded.host,
               pname = excluded.pname,
               started_at_ms = excluded.started_at_ms",
            params![
                event.drv_path,
                event.host,
                pname,
                timestamp_to_i64(event.timestamp_ms)?
            ],
        )
        .map_err(sqlite_error)?;
    } else if event.kind == "finish" {
        let tx = conn.transaction().map_err(sqlite_error)?;
        let start = tx
            .query_row(
                "SELECT host, pname, started_at_ms FROM active_builds WHERE drv_path = ?1",
                params![event.drv_path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite_error)?;

        tx.execute(
            "DELETE FROM active_builds WHERE drv_path = ?1",
            params![event.drv_path],
        )
        .map_err(sqlite_error)?;

        if let Some((start_host, start_pname, start_ms)) = start {
            let start_ms_u128 = start_ms.max(0) as u128;
            let duration_ms = event.timestamp_ms.saturating_sub(start_ms_u128);
            tx.execute(
                "INSERT INTO build_observations
                   (host, pname, drv_path, started_at_ms, finished_at_ms, duration_ms, status, out_paths)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    start_host,
                    start_pname,
                    event.drv_path,
                    start_ms,
                    timestamp_to_i64(event.timestamp_ms)?,
                    duration_to_i64(duration_ms)?,
                    event.status,
                    event.out_paths,
                ],
            )
            .map_err(sqlite_error)?;
            prune_pname_samples(&tx, &start_pname, cfg.max_samples_per_pname)?;
            eprintln!(
                "build_finished host={} pname={} duration_ms={} status={}",
                event.host, pname, duration_ms, event.status
            );
        } else {
            eprintln!(
                "build_finish_unmatched host={} pname={} status={}",
                event.host, pname, event.status
            );
        }
        tx.commit().map_err(sqlite_error)?;
    }
    Ok(())
}

/// Decide whether one build candidate should run on the configured remote host.
///
/// This is the scheduler's top-level pipeline: load local and remote state,
/// apply hard safety checks, compare completion predictions, and persist remote
/// admission only when the final decision accepts remote execution.
fn decide_build_candidate(cfg: &Config, candidate: &BuildCandidate) -> io::Result<Decision> {
    let scheduler = SchedulerConfig::from_candidate(cfg, candidate);
    let conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_admissions_with_policy(&conn, &scheduler.policy)?;
    let decision_time_ms = now_ms();

    if let Eligibility::Declined { reason } = evaluate_candidate_compatibility(candidate) {
        return Ok(decline(reason));
    }

    let local = load_local_host_state(&conn, candidate, &scheduler, decision_time_ms)?;
    let remote = load_remote_host_state(&conn, cfg, candidate, &scheduler.remote_target)?;

    if let Eligibility::Declined { reason } =
        evaluate_remote_health(&remote, decision_time_ms, &scheduler.policy)
    {
        return Ok(decline(reason));
    }

    let (local_prediction, remote_prediction, unknown) =
        evaluate_predictions(&local, &remote, &scheduler);

    if let Eligibility::Declined { reason } =
        evaluate_remote_admission_limits(&remote, unknown, decision_time_ms, &scheduler.policy)
    {
        return Ok(decline(reason));
    }

    let metrics = decision_metrics(&local, &remote, &local_prediction, &remote_prediction);
    let outcome = compare_predictions(
        candidate,
        &scheduler.remote_target,
        metrics,
        &scheduler.policy,
    );

    if outcome.record_remote_admission {
        record_remote_admission_at(
            &conn,
            candidate,
            remote_prediction.package_ms,
            unknown,
            decision_time_ms,
        )?;
    }

    Ok(outcome.decision)
}

/// Check whether the candidate can be considered by the current single target.
///
/// This does not yet inspect per-target feature declarations because the tool
/// only has one configured remote builder.
fn evaluate_candidate_compatibility(candidate: &BuildCandidate) -> Eligibility {
    if candidate.needed_system == "x86_64-linux" || candidate.needed_system == "builtin" {
        Eligibility::Accepted
    } else {
        Eligibility::Declined {
            reason: "unsupported system",
        }
    }
}

/// Load live local telemetry and local package history for a candidate.
///
/// Local queue state combines Nix's current slot files with locally observed
/// active builds recorded by the pre-build hook.
fn load_local_host_state(
    conn: &Connection,
    candidate: &BuildCandidate,
    scheduler: &SchedulerConfig,
    now: u128,
) -> io::Result<HostState> {
    let telemetry = read_telemetry(&scheduler.local_host_name)?;
    let stats = local_package_stats_from_conn(conn, &candidate.pname)?;
    let (active_count, active_queue_ms) =
        active_local_queue_ms(conn, &scheduler.local_host_name, now, &scheduler.policy)?;
    Ok(HostState {
        telemetry,
        stats,
        active_count,
        active_queue_ms,
        admissions: Vec::new(),
    })
}

/// Load cached remote telemetry, cached package stats, and active admissions.
///
/// Remote data comes from the controller's polling cache. Missing or stale
/// files are handled by later fail-closed scheduler checks.
fn load_remote_host_state(
    conn: &Connection,
    cfg: &Config,
    candidate: &BuildCandidate,
    target: &BuildTarget,
) -> io::Result<HostState> {
    let telemetry = read_remote_telemetry(&cfg.data_dir, &target.host_name)?;
    let stats = remote_package_stats(&cfg.data_dir, &target.host_name, &candidate.pname)?;
    let admissions = remote_admissions(conn, &target.host_name)?;
    Ok(HostState {
        telemetry,
        stats,
        active_count: 0,
        active_queue_ms: 0,
        admissions,
    })
}

/// Apply hard remote health checks before considering queue predictions.
fn evaluate_remote_health(remote: &HostState, now: u128, policy: &SchedulerPolicy) -> Eligibility {
    if now.saturating_sub(remote.telemetry.timestamp_ms) > policy.stale_telemetry_ms {
        return Eligibility::Declined {
            reason: "remote telemetry is stale",
        };
    }
    if remote.telemetry.cpu_busy_ratio.unwrap_or(1.0) > policy.max_remote_cpu_busy_ratio {
        return Eligibility::Declined {
            reason: "remote cpu is busy",
        };
    }
    if remote.telemetry.psi_memory_some_avg10.unwrap_or(0.0)
        > policy.max_remote_memory_pressure_avg10
    {
        return Eligibility::Declined {
            reason: "remote memory pressure is high",
        };
    }
    if remote.telemetry.mem_available_kb.unwrap_or(0) < policy.min_remote_mem_available_kb {
        return Eligibility::Declined {
            reason: "remote memory is low",
        };
    }
    Eligibility::Accepted
}

/// Build local and remote completion predictions for one candidate.
///
/// Package duration and queue delay are kept separate so logs and tests can
/// explain why a decision was made.
fn evaluate_predictions(
    local: &HostState,
    remote: &HostState,
    scheduler: &SchedulerConfig,
) -> (Prediction, Prediction, bool) {
    let (local_package_ms, remote_package_ms) = paired_predictions_with_policy(
        local.stats.as_ref(),
        remote.stats.as_ref(),
        &scheduler.policy,
    );
    let local_samples = local.stats.as_ref().map(|stats| stats.count).unwrap_or(0);
    let remote_samples = remote.stats.as_ref().map(|stats| stats.count).unwrap_or(0);
    let local_queue_ms = local_queue_ms(local, &scheduler.policy);
    let remote_queue_ms = remote_queue_ms(remote, &scheduler.remote_target, &scheduler.policy);
    let local_prediction = Prediction {
        samples: local_samples,
        package_ms: local_package_ms,
        queue_ms: local_queue_ms,
        completion_ms: local_queue_ms + local_package_ms,
    };
    let remote_prediction = Prediction {
        samples: remote_samples,
        package_ms: remote_package_ms,
        queue_ms: remote_queue_ms,
        completion_ms: remote_queue_ms + remote_package_ms,
    };
    let unknown = local_samples == 0 && remote_samples == 0;
    (local_prediction, remote_prediction, unknown)
}

/// Apply limits that prevent the hook from flooding the remote builder.
fn evaluate_remote_admission_limits(
    remote: &HostState,
    unknown: bool,
    now: u128,
    policy: &SchedulerPolicy,
) -> Eligibility {
    if remote.admissions.len() >= policy.max_remote_admitted {
        return Eligibility::Declined {
            reason: "remote admission limit reached",
        };
    }
    if unknown
        && remote
            .admissions
            .iter()
            .filter(|admission| admission.unknown)
            .count()
            >= policy.max_unknown_remote
    {
        return Eligibility::Declined {
            reason: "unknown remote admission limit reached",
        };
    }
    if let Some(last) = remote
        .admissions
        .iter()
        .map(|admission| admission.admitted_at_ms)
        .max()
    {
        if now.saturating_sub(last as u128) < policy.min_remote_admission_interval_ms {
            return Eligibility::Declined {
                reason: "remote admission interval not elapsed",
            };
        }
    }
    Eligibility::Accepted
}

/// Estimate how long a new local build would wait behind current local work.
fn local_queue_ms(local: &HostState, policy: &SchedulerPolicy) -> u64 {
    let local_slot_queue_ms = (local.telemetry.nix_slots_local as u64 * policy.unknown_p95_ms)
        / policy.local_capacity as u64;
    local_slot_queue_ms.max(local.active_queue_ms)
}

/// Estimate how long a remote build would wait behind current remote work.
///
/// Remote queueing includes both Nix slots reported by the agent and
/// controller-side admissions that have not yet reported completion.
fn remote_queue_ms(remote: &HostState, target: &BuildTarget, policy: &SchedulerPolicy) -> u64 {
    let remote_existing_ms =
        (remote.telemetry.nix_slots_total as u64 * policy.unknown_p95_ms) / target.capacity as u64;
    let admitted_ms: u64 = remote
        .admissions
        .iter()
        .map(|admission| admission.predicted_ms)
        .sum();
    remote_existing_ms + admitted_ms / target.capacity as u64
}

/// Collect the scheduler fields that are useful for logs and regression tests.
fn decision_metrics(
    local: &HostState,
    remote: &HostState,
    local_prediction: &Prediction,
    remote_prediction: &Prediction,
) -> DecisionMetrics {
    DecisionMetrics {
        local_samples: local_prediction.samples,
        remote_samples: remote_prediction.samples,
        local_prediction_ms: local_prediction.package_ms,
        remote_prediction_ms: remote_prediction.package_ms,
        local_queue_ms: local_prediction.queue_ms,
        remote_queue_ms: remote_prediction.queue_ms,
        local_completion_ms: local_prediction.completion_ms,
        remote_completion_ms: remote_prediction.completion_ms,
        local_slots: local.telemetry.nix_slots_local,
        remote_slots: remote.telemetry.nix_slots_total,
        local_active_count: local.active_count,
        admitted_count: remote.admissions.len(),
    }
}

/// Choose local or remote after all hard eligibility checks have passed.
///
/// Remote wins only when its predicted completion is faster, except that the
/// bounded exploration policy may choose an empty host to refresh sparse data.
fn compare_predictions(
    candidate: &BuildCandidate,
    target: &BuildTarget,
    metrics: DecisionMetrics,
    policy: &SchedulerPolicy,
) -> DecisionOutcome {
    let explore = should_explore_empty_host(candidate, &metrics, policy);

    if metrics.remote_completion_ms >= metrics.local_completion_ms {
        if explore && remote_host_is_empty(&metrics) {
            return DecisionOutcome {
                record_remote_admission: true,
                decision: Decision {
                    decision: "accept".to_string(),
                    reason: "exploration: empty remote host selected".to_string(),
                    store_uri: Some(target.store_uri.clone()),
                    metrics: Some(metrics),
                },
            };
        }
        return DecisionOutcome {
            record_remote_admission: false,
            decision: Decision {
                decision: "decline".to_string(),
                reason: "local queue is predicted to drain sooner".to_string(),
                store_uri: None,
                metrics: Some(metrics),
            },
        };
    }

    if explore && local_host_is_empty(&metrics) {
        return DecisionOutcome {
            record_remote_admission: false,
            decision: Decision {
                decision: "decline".to_string(),
                reason: "exploration: empty local host selected".to_string(),
                store_uri: None,
                metrics: Some(metrics),
            },
        };
    }

    DecisionOutcome {
        record_remote_admission: true,
        decision: Decision {
            decision: "accept".to_string(),
            reason: format!(
                "remote predicted {}ms vs local {}ms",
                metrics.remote_completion_ms, metrics.local_completion_ms
            ),
            store_uri: Some(target.store_uri.clone()),
            metrics: Some(metrics),
        },
    }
}

/// Record an accepted remote decision until the hook reports completion.
///
/// The admission table is a queue-accounting aid. It is intentionally updated
/// only after all scheduler checks accept the candidate.
fn record_remote_admission_at(
    conn: &Connection,
    candidate: &BuildCandidate,
    remote_prediction: u64,
    unknown: bool,
    admitted_at_ms: u128,
) -> io::Result<()> {
    conn.execute(
        "INSERT INTO remote_admissions
           (drv_path, host, pname, admitted_at_ms, predicted_ms, unknown)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(drv_path) DO UPDATE SET
           host = excluded.host,
           pname = excluded.pname,
           admitted_at_ms = excluded.admitted_at_ms,
           predicted_ms = excluded.predicted_ms,
           unknown = excluded.unknown",
        params![
            candidate.drv_path,
            candidate.remote_host,
            candidate.pname,
            timestamp_to_i64(admitted_at_ms)?,
            duration_to_i64(remote_prediction as u128)?,
            if unknown { 1 } else { 0 },
        ],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// Return whether a sparse-history candidate falls into the stable exploration bucket.
fn should_explore_empty_host(
    candidate: &BuildCandidate,
    metrics: &DecisionMetrics,
    policy: &SchedulerPolicy,
) -> bool {
    if metrics.local_samples >= policy.exploration_min_samples
        && metrics.remote_samples >= policy.exploration_min_samples
    {
        return false;
    }
    stable_percent(&candidate.drv_path) < policy.exploration_percent
}

fn local_host_is_empty(metrics: &DecisionMetrics) -> bool {
    metrics.local_slots == 0 && metrics.local_active_count == 0
}

fn remote_host_is_empty(metrics: &DecisionMetrics) -> bool {
    metrics.remote_slots == 0 && metrics.admitted_count == 0
}

fn stable_percent(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash % 100
}

fn decline(reason: &str) -> Decision {
    Decision {
        decision: "decline".to_string(),
        reason: reason.to_string(),
        store_uri: None,
        metrics: None,
    }
}

#[derive(Debug)]
struct Admission {
    admitted_at_ms: i64,
    predicted_ms: u64,
    unknown: bool,
}

/// Load active remote admissions for one host in admission order.
fn remote_admissions(conn: &Connection, remote_host: &str) -> io::Result<Vec<Admission>> {
    let mut stmt = conn
        .prepare(
            "SELECT admitted_at_ms, predicted_ms, unknown FROM remote_admissions
             WHERE host = ?1
             ORDER BY admitted_at_ms",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map(params![remote_host], |row| {
            let predicted_ms = row.get::<_, i64>(1)?.max(0) as u64;
            Ok(Admission {
                admitted_at_ms: row.get(0)?,
                predicted_ms,
                unknown: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(sqlite_error)?;
    let mut admissions = Vec::new();
    for row in rows {
        admissions.push(row.map_err(sqlite_error)?);
    }
    Ok(admissions)
}

/// Remove admissions old enough that the delegated build likely disappeared.
///
/// Normal completions remove admissions by derivation path. This cleanup keeps
/// the scheduler from permanently reserving capacity after hook or daemon loss.
fn cleanup_stale_admissions_with_policy(
    conn: &Connection,
    policy: &SchedulerPolicy,
) -> io::Result<()> {
    let cutoff = timestamp_to_i64(now_ms().saturating_sub(policy.unknown_p95_ms as u128 * 2))?;
    conn.execute(
        "DELETE FROM remote_admissions WHERE admitted_at_ms < ?1",
        params![cutoff],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// Mark a previously admitted remote derivation as no longer queued or running.
fn finish_admission(data_dir: &Path, drv_path: &str) -> io::Result<()> {
    let conn = open_history_db(data_dir)?;
    conn.execute(
        "DELETE FROM remote_admissions WHERE drv_path = ?1",
        params![drv_path],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

#[cfg(test)]
fn paired_predictions(
    local_stats: Option<&PackageStats>,
    remote_stats: Option<&PackageStats>,
) -> (u64, u64) {
    paired_predictions_with_policy(local_stats, remote_stats, &SchedulerPolicy::default())
}

/// Predict package durations for local and remote from available p95 samples.
///
/// If only one host has history for the package, both hosts use that estimate.
/// This avoids treating missing remote history as proof that remote is slow or
/// missing local history as proof that local is slow.
fn paired_predictions_with_policy(
    local_stats: Option<&PackageStats>,
    remote_stats: Option<&PackageStats>,
    policy: &SchedulerPolicy,
) -> (u64, u64) {
    let local = sample_prediction_ms(local_stats);
    let remote = sample_prediction_ms(remote_stats);
    (
        local.or(remote).unwrap_or(policy.unknown_p95_ms),
        remote.or(local).unwrap_or(policy.unknown_p95_ms),
    )
}

/// Convert package stats into a non-zero duration prediction.
fn sample_prediction_ms(stats: Option<&PackageStats>) -> Option<u64> {
    stats.and_then(|stats| (stats.count > 0).then(|| stats.p95_ms.max(1)))
}

/// Read local successful build durations for one package and return p95 stats.
fn local_package_stats_from_conn(
    conn: &Connection,
    pname: &str,
) -> io::Result<Option<PackageStats>> {
    let mut stmt = conn
        .prepare(
            "SELECT duration_ms FROM build_observations
             WHERE status = 'success' AND pname = ?1
             ORDER BY duration_ms",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map(params![pname], |row| row.get::<_, i64>(0))
        .map_err(sqlite_error)?;
    let mut values = Vec::new();
    for row in rows {
        let duration = row.map_err(sqlite_error)?;
        if duration >= 0 {
            values.push(duration as u64);
        }
    }
    Ok((!values.is_empty()).then(|| PackageStats {
        count: values.len() as u64,
        p95_ms: quantile(&values, 0.95).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
    }))
}

/// Estimate remaining local work from active build starts recorded in SQLite.
///
/// Each active build contributes its predicted remaining duration, divided by
/// local capacity to approximate parallel slot drain time.
fn active_local_queue_ms(
    conn: &Connection,
    host: &str,
    now: u128,
    policy: &SchedulerPolicy,
) -> io::Result<(usize, u64)> {
    let mut stmt = conn
        .prepare(
            "SELECT pname, started_at_ms FROM active_builds
             WHERE host = ?1",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map(params![host], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(sqlite_error)?;
    let mut active = Vec::new();
    for row in rows {
        active.push(row.map_err(sqlite_error)?);
    }
    drop(stmt);

    let mut remaining_ms = 0u64;
    let active_count = active.len();
    for (pname, started_at_ms) in active {
        let stats = local_package_stats_from_conn(conn, &pname)?;
        let prediction = sample_prediction_ms(stats.as_ref()).unwrap_or(policy.unknown_p95_ms);
        let elapsed_ms = now.saturating_sub(started_at_ms.max(0) as u128) as u64;
        remaining_ms = remaining_ms.saturating_add(prediction.saturating_sub(elapsed_ms));
    }

    Ok((active_count, remaining_ms / policy.local_capacity as u64))
}

/// Read one package's latest cached stats from a polled remote agent.
fn remote_package_stats(
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

/// Read cached telemetry for a remote host from the controller data directory.
fn read_remote_telemetry(data_dir: &Path, remote_host: &str) -> io::Result<Telemetry> {
    let text = fs::read_to_string(data_dir.join(format!("telemetry-{remote_host}.json")))?;
    let mut telemetry: Telemetry = serde_json::from_str(&text).map_err(json_error)?;
    if telemetry.host.is_empty() {
        telemetry.host = remote_host.to_string();
    }
    Ok(telemetry)
}

fn cleanup_state(cfg: &Config) -> io::Result<()> {
    let conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_starts(&conn, cfg.stale_start_ms)
}

/// Open the history database and ensure the current schema exists.
fn open_history_db(data_dir: &Path) -> io::Result<Connection> {
    fs::create_dir_all(data_dir)?;
    let conn = Connection::open(data_dir.join("history.sqlite3")).map_err(sqlite_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(sqlite_error)?;
    init_history_schema(&conn)?;
    Ok(conn)
}

/// Create the SQLite schema used by build observation and admission tracking.
fn init_history_schema(conn: &Connection) -> io::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS meta (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS active_builds (
          drv_path TEXT PRIMARY KEY,
          host TEXT NOT NULL,
          pname TEXT NOT NULL,
          started_at_ms INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS build_observations (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          host TEXT NOT NULL,
          pname TEXT NOT NULL,
          drv_path TEXT NOT NULL,
          started_at_ms INTEGER NOT NULL,
          finished_at_ms INTEGER NOT NULL,
          duration_ms INTEGER NOT NULL,
          status TEXT NOT NULL,
          out_paths TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_build_observations_pname_finished
          ON build_observations (pname, finished_at_ms DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_build_observations_success_pname
          ON build_observations (pname, duration_ms)
          WHERE status = 'success';
        CREATE TABLE IF NOT EXISTS remote_admissions (
          drv_path TEXT PRIMARY KEY,
          host TEXT NOT NULL,
          pname TEXT NOT NULL,
          admitted_at_ms INTEGER NOT NULL,
          predicted_ms INTEGER NOT NULL,
          unknown INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_admissions_host
          ON remote_admissions (host, admitted_at_ms);
        ",
    )
    .map_err(sqlite_error)?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// Remove pre-build starts that never received a matching post-build event.
fn cleanup_stale_starts(conn: &Connection, stale_start_ms: u128) -> io::Result<()> {
    if stale_start_ms == 0 {
        return Ok(());
    }
    let cutoff = timestamp_to_i64(now_ms().saturating_sub(stale_start_ms))?;
    let removed = conn
        .execute(
            "DELETE FROM active_builds WHERE started_at_ms < ?1",
            params![cutoff],
        )
        .map_err(sqlite_error)?;
    if removed > 0 {
        eprintln!("stale_starts_removed count={removed}");
    }
    Ok(())
}

/// Keep only the newest successful and failed observations for one package.
///
/// Retention is per `pname` so common packages cannot evict all history for
/// rarely built packages.
fn prune_pname_samples(
    conn: &Connection,
    pname: &str,
    max_samples_per_pname: usize,
) -> io::Result<()> {
    if max_samples_per_pname == 0 {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM build_observations
         WHERE pname = ?1
           AND id NOT IN (
             SELECT id FROM build_observations
             WHERE pname = ?1
             ORDER BY finished_at_ms DESC, id DESC
             LIMIT ?2
           )",
        params![pname, max_samples_per_pname as i64],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// Sample the local host telemetry used by agents and scheduler decisions.
fn read_telemetry(host: &str) -> io::Result<Telemetry> {
    let (cpu_busy_ratio, _) = read_cpu_busy_ratio()?;
    let (mem_total_kb, mem_available_kb) = read_meminfo()?;
    let psi_memory_some_avg10 = read_psi_memory_some_avg10().ok().flatten();
    let (nix_slots_total, nix_slots_local, nix_slots_remote) = read_nix_slots();

    Ok(Telemetry {
        host: host.to_string(),
        timestamp_ms: now_ms(),
        cpu_busy_ratio,
        mem_total_kb,
        mem_available_kb,
        psi_memory_some_avg10,
        nix_slots_total,
        nix_slots_local,
        nix_slots_remote,
    })
}

/// Estimate CPU busy ratio from two `/proc/stat` samples.
fn read_cpu_busy_ratio() -> io::Result<(Option<f64>, CpuSample)> {
    let first = read_cpu_sample()?;
    thread::sleep(Duration::from_millis(100));
    let second = read_cpu_sample()?;
    let total_delta = second.total.saturating_sub(first.total);
    let idle_delta = second.idle.saturating_sub(first.idle);
    let busy = if total_delta == 0 {
        None
    } else {
        Some(1.0 - (idle_delta as f64 / total_delta as f64))
    };
    Ok((busy, second))
}

#[derive(Debug)]
struct CpuSample {
    idle: u64,
    total: u64,
}

fn read_cpu_sample() -> io::Result<CpuSample> {
    let cpu = KernelStats::current().map_err(proc_error)?.total;
    let idle = cpu.idle + cpu.iowait.unwrap_or(0);
    let total = cpu.user
        + cpu.nice
        + cpu.system
        + cpu.idle
        + cpu.iowait.unwrap_or(0)
        + cpu.irq.unwrap_or(0)
        + cpu.softirq.unwrap_or(0)
        + cpu.steal.unwrap_or(0)
        + cpu.guest.unwrap_or(0)
        + cpu.guest_nice.unwrap_or(0);
    Ok(CpuSample { idle, total })
}

fn read_meminfo() -> io::Result<(Option<u64>, Option<u64>)> {
    let meminfo = Meminfo::current().map_err(proc_error)?;
    Ok((
        Some(meminfo.mem_total / 1024),
        meminfo.mem_available.map(|v| v / 1024),
    ))
}

fn read_psi_memory_some_avg10() -> io::Result<Option<f64>> {
    let pressure = MemoryPressure::current().map_err(proc_error)?;
    Ok(Some(pressure.some.avg10.into()))
}

/// Count currently locked Nix build slot files.
///
/// Nix represents active local and remote builds as locked files in
/// `/nix/var/nix/current-load`; unlocked files are stale and ignored.
fn read_nix_slots() -> (usize, usize, usize) {
    let dir = Path::new("/nix/var/nix/current-load");
    let mut total = 0;
    let mut local = 0;
    let mut remote = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0, 0);
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "main-lock" || name.ends_with(".upload-lock") {
            continue;
        }
        if !slot_file_is_locked(&entry.path()) {
            continue;
        }
        total += 1;
        if name.contains("localhost") {
            local += 1;
        } else if name.starts_with("ssh") {
            remote += 1;
        }
    }
    (total, local, remote)
}

/// Return whether a Nix slot file is currently locked by another process.
///
/// A successful non-blocking exclusive lock means the file was stale, so this
/// function unlocks it and reports `false`.
fn slot_file_is_locked(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    let fd = file.as_raw_fd();
    let result = try_flock_exclusive(fd);
    if result {
        let _ = unlock_flock(fd);
        false
    } else {
        true
    }
}

fn try_flock_exclusive(fd: RawFd) -> bool {
    // SAFETY: `fd` comes from a live `File` in `slot_file_is_locked`, and `flock`
    // does not take ownership of the descriptor.
    unsafe { flock(fd, LOCK_EX | LOCK_NB) == 0 }
}

fn unlock_flock(fd: RawFd) -> io::Result<()> {
    // SAFETY: `fd` comes from a live `File` in `slot_file_is_locked`, and `flock`
    // does not take ownership of the descriptor.
    if unsafe { flock(fd, LOCK_UN) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn telemetry_json(telemetry: &Telemetry) -> String {
    json_line(telemetry)
}

/// Render local successful build history as the remote-agent `/stats` response.
fn stats_json(data_dir: &Path) -> io::Result<String> {
    let conn = open_history_db(data_dir)?;
    let mut durations: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT pname, duration_ms FROM build_observations
             WHERE status = 'success'
             ORDER BY pname",
        )
        .map_err(sqlite_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(sqlite_error)?;
    for row in rows {
        let (pname, duration) = row.map_err(sqlite_error)?;
        if duration >= 0 {
            durations.entry(pname).or_default().push(duration as u64);
        }
    }

    let mut packages = Vec::new();
    for (pname, mut values) in durations {
        values.sort_unstable();
        let count = values.len();
        packages.push(PackageStatsEntry {
            pname,
            count: count as u64,
            p50_ms: quantile(&values, 0.50).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
            p80_ms: quantile(&values, 0.80).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
            p95_ms: quantile(&values, 0.95).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
        });
    }

    Ok(json_line(&StatsResponse {
        unknown_p95_ms: DEFAULT_UNKNOWN_P95_MS,
        packages,
    }))
}

/// Return the upper-bucket quantile for already sorted duration values.
fn quantile(values: &[u64], q: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let idx = ((values.len() - 1) as f64 * q).ceil() as usize;
    values.get(idx).copied()
}

fn timestamp_to_i64(value: u128) -> io::Result<i64> {
    i64::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "timestamp is too large"))
}

fn duration_to_i64(value: u128) -> io::Result<i64> {
    i64::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "duration is too large"))
}

fn sqlite_error(err: rusqlite::Error) -> io::Error {
    io::Error::other(err)
}

fn proc_error(err: procfs::ProcError) -> io::Error {
    io::Error::other(err)
}

/// Poll configured remote agents and cache their latest telemetry and stats.
fn poll_remotes(cfg: Config) {
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

fn get_http_tcp(addr: &str, path: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(addr)?;
    let request = format!("GET {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n");
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let Some((headers, body)) = response.split_once("\r\n\r\n") else {
        return invalid("invalid HTTP response");
    };
    if headers.starts_with("HTTP/1.1 200 ") {
        Ok(body.to_string())
    } else {
        Err(io::Error::other(response))
    }
}

/// Extract a normalized package name from a Nix derivation path.
///
/// The store hash and `.drv` suffix are removed before version-like suffix
/// components are stripped.
fn pname_from_drv(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .strip_suffix(".drv")
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
    let without_hash = name.split_once('-').map(|(_, rest)| rest).unwrap_or(name);
    normalize_pname(without_hash)
}

/// Strip trailing version-like components from a derivation output name.
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

/// Return whether a hyphen-separated name component looks like a version.
fn looks_versionish(part: &str) -> bool {
    part.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || part
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '.' || ch == '_' || ch == '+')
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn hostname_fallback() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "nix-build-balancer-test-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn test_config(data_dir: PathBuf) -> Config {
        Config {
            mode: Mode::Agent,
            host: "test-host".to_string(),
            data_dir,
            unix_socket: None,
            listen: None,
            remote: Vec::new(),
            poll_interval: Duration::from_secs(1),
            max_samples_per_pname: 200,
            stale_start_ms: 0,
            once: true,
        }
    }

    fn write_remote_telemetry(dir: &Path, timestamp_ms: u128, cpu_busy_ratio: f64) {
        write_remote_telemetry_full(dir, timestamp_ms, cpu_busy_ratio, 64_000_000, 0.0, 0);
    }

    fn write_remote_telemetry_full(
        dir: &Path,
        timestamp_ms: u128,
        cpu_busy_ratio: f64,
        mem_available_kb: u64,
        psi_memory_some_avg10: f64,
        nix_slots_total: usize,
    ) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("telemetry-tsugumi.json"),
            format!(
                "{{\"host\":\"tsugumi\",\"timestamp_ms\":{},\"cpu_busy_ratio\":{},\"mem_total_kb\":130000000,\"mem_available_kb\":{},\"psi_memory_some_avg10\":{},\"nix_slots_total\":{},\"nix_slots_local\":0,\"nix_slots_remote\":0}}\n",
                timestamp_ms,
                cpu_busy_ratio,
                mem_available_kb,
                psi_memory_some_avg10,
                nix_slots_total
            ),
        )
        .unwrap();
    }

    fn write_remote_stats(dir: &Path, pname: &str, count: u64, p95_ms: u64) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("stats-tsugumi.json"),
            format!(
                "{{\"unknown_p95_ms\":1800000,\"packages\":[{{\"pname\":{},\"count\":{},\"p50_ms\":{},\"p80_ms\":{},\"p95_ms\":{}}}]}}\n",
                serde_json::to_string(pname).unwrap(),
                count,
                p95_ms,
                p95_ms,
                p95_ms
            ),
        )
        .unwrap();
    }

    fn exploration_drv_path(prefix: &str) -> String {
        (0..1_000)
            .map(|idx| format!("/nix/store/hash-{prefix}-{idx}.drv"))
            .find(|path| stable_percent(path) < DEFAULT_EXPLORATION_PERCENT)
            .expect("test should find an exploration bucket")
    }

    fn build_event(kind: &str, drv_path: &str, timestamp_ms: u128, status: &str) -> BuildEvent {
        BuildEvent {
            kind: kind.to_string(),
            drv_path: drv_path.to_string(),
            out_paths: "/nix/store/out".to_string(),
            status: status.to_string(),
            host: "test-host".to_string(),
            timestamp_ms,
        }
    }

    fn test_candidate(drv_path: &str) -> BuildCandidate {
        BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: drv_path.to_string(),
            required_features: Vec::new(),
            pname: pname_from_drv(drv_path),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        }
    }

    fn non_exploration_drv_path(prefix: &str) -> String {
        (0..1_000)
            .map(|idx| format!("/nix/store/hash-{prefix}-{idx}.drv"))
            .find(|path| stable_percent(path) >= DEFAULT_EXPLORATION_PERCENT)
            .expect("test should find a non-exploration bucket")
    }

    fn test_target() -> BuildTarget {
        BuildTarget {
            host_name: DEFAULT_REMOTE_HOST.to_string(),
            store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
            capacity: DEFAULT_REMOTE_CAPACITY,
        }
    }

    fn test_metrics(local_completion_ms: u64, remote_completion_ms: u64) -> DecisionMetrics {
        DecisionMetrics {
            local_samples: DEFAULT_EXPLORATION_MIN_SAMPLES,
            remote_samples: DEFAULT_EXPLORATION_MIN_SAMPLES,
            local_prediction_ms: local_completion_ms,
            remote_prediction_ms: remote_completion_ms,
            local_queue_ms: 0,
            remote_queue_ms: 0,
            local_completion_ms,
            remote_completion_ms,
            local_slots: 0,
            remote_slots: 0,
            local_active_count: 0,
            admitted_count: 0,
        }
    }

    #[test]
    fn clap_parses_serve_defaults_and_validation() {
        let command = Cli::try_parse_from(["nbb", "serve", "--once"])
            .unwrap()
            .command
            .unwrap();
        let CliCommand::Serve(args) = command else {
            panic!("expected serve command");
        };
        let cfg = serve_config(args, true).unwrap();

        assert!(matches!(cfg.mode, Mode::Agent));
        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/nix-build-balancer"));
        assert_eq!(cfg.poll_interval, Duration::from_secs(1));
        assert!(cfg.once);

        let command = Cli::try_parse_from(["nbb", "serve"])
            .unwrap()
            .command
            .unwrap();
        let CliCommand::Serve(args) = command else {
            panic!("expected serve command");
        };
        assert!(serve_config(args, true).is_err());
    }

    #[test]
    fn clap_parses_event_and_hook_args() {
        let command = Cli::try_parse_from([
            "nbb",
            "event",
            "--endpoint",
            "unix:/tmp/nbb.sock",
            "--kind",
            "start",
            "--drv-path",
            "/nix/store/hash-kwin-6.6.3.drv",
        ])
        .unwrap()
        .command
        .unwrap();
        let CliCommand::Event(args) = command else {
            panic!("expected event command");
        };
        let (endpoint, event): (String, BuildEvent) = args.into();
        assert_eq!(endpoint, "unix:/tmp/nbb.sock");
        assert_eq!(event.kind, "start");
        assert_eq!(event.status, "unknown");

        let command = Cli::try_parse_from([
            "nbb",
            "hook",
            "--endpoint",
            "unix:/tmp/nbb.sock",
            "--remote-host",
            "builder",
            "--",
            "-vv",
        ])
        .unwrap()
        .command
        .unwrap();
        let CliCommand::Hook(args) = command else {
            panic!("expected hook command");
        };
        let cfg = HookConfig::try_from(args).unwrap();
        assert_eq!(cfg.endpoint, "unix:/tmp/nbb.sock");
        assert_eq!(cfg.remote_host, "builder");
        assert_eq!(cfg.verbosity, "-vv");
    }

    fn insert_remote_admission(
        conn: &Connection,
        drv_path: &str,
        admitted_at_ms: u128,
        predicted_ms: u64,
        unknown: bool,
    ) {
        conn.execute(
            "INSERT INTO remote_admissions
               (drv_path, host, pname, admitted_at_ms, predicted_ms, unknown)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                drv_path,
                DEFAULT_REMOTE_HOST,
                pname_from_drv(drv_path),
                timestamp_to_i64(admitted_at_ms).unwrap(),
                duration_to_i64(predicted_ms as u128).unwrap(),
                if unknown { 1 } else { 0 },
            ],
        )
        .unwrap();
    }

    fn remote_admission_count(dir: &Path) -> i64 {
        let conn = open_history_db(dir).unwrap();
        conn.query_row("SELECT count(*) FROM remote_admissions", [], |row| {
            row.get(0)
        })
        .unwrap()
    }

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

    #[test]
    fn quantiles_use_upper_bucket() {
        let values = [10, 20, 30, 40];
        assert_eq!(quantile(&values, 0.50), Some(30));
        assert_eq!(quantile(&values, 0.95), Some(40));
    }

    #[test]
    fn records_successful_build_stats_from_sqlite() {
        let dir = test_data_dir("stats");
        let cfg = test_config(dir.clone());
        let drv = "/nix/store/hash-kwin-6.6.3.drv";

        record_event(&cfg, &build_event("start", drv, 1_000, "unknown")).unwrap();
        record_event(&cfg, &build_event("finish", drv, 2_500, "success")).unwrap();

        let stats = stats_json(&dir).unwrap();
        assert!(stats.contains("\"pname\":\"kwin\""));
        assert!(stats.contains("\"count\":1"));
        assert!(stats.contains("\"p50_ms\":1500"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stores_failures_but_excludes_them_from_stats() {
        let dir = test_data_dir("failure");
        let cfg = test_config(dir.clone());
        let drv = "/nix/store/hash-failing-package-1.0.drv";

        record_event(&cfg, &build_event("start", drv, 1_000, "unknown")).unwrap();
        record_event(&cfg, &build_event("finish", drv, 2_000, "failure")).unwrap();

        let conn = open_history_db(&dir).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM build_observations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        let stats = stats_json(&dir).unwrap();
        assert!(!stats.contains("failing-package"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn retention_is_per_pname() {
        let dir = test_data_dir("retention");
        let mut cfg = test_config(dir.clone());
        cfg.max_samples_per_pname = 2;

        for i in 0..3 {
            let drv = format!("/nix/store/hash-kwin-6.6.{i}.drv");
            record_event(
                &cfg,
                &build_event("start", &drv, 1_000 + i * 1_000, "unknown"),
            )
            .unwrap();
            record_event(
                &cfg,
                &build_event("finish", &drv, 1_500 + i * 1_000, "success"),
            )
            .unwrap();
        }

        let other_drv = "/nix/store/hash-linux-6.19.5.drv";
        record_event(&cfg, &build_event("start", other_drv, 10_000, "unknown")).unwrap();
        record_event(&cfg, &build_event("finish", other_drv, 11_000, "success")).unwrap();

        let conn = open_history_db(&dir).unwrap();
        let kwin_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM build_observations WHERE pname = 'kwin'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let linux_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM build_observations WHERE pname = 'linux'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kwin_count, 2);
        assert_eq!(linux_count, 1);
        let _ = fs::remove_dir_all(dir);
    }

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

    #[test]
    fn stale_slot_files_are_not_active_slots() {
        let dir = test_data_dir("slot");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ssh-ng:__svein@tsugumi.local-0");
        fs::write(&path, "").unwrap();

        assert!(!slot_file_is_locked(&path));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_package_stats_from_remote_json() {
        let stats = r#"{"unknown_p95_ms":1800000,"packages":[{"pname":"kwin","count":3,"p50_ms":10,"p80_ms":20,"p95_ms":30}]}"#;
        let parsed = parse_package_stats(stats, "kwin").unwrap();
        assert_eq!(parsed.count, 3);
        assert_eq!(parsed.p95_ms, 30);
        assert!(parse_package_stats(stats, "missing").is_none());
    }

    #[test]
    fn paired_predictions_borrow_missing_side_history() {
        let local = PackageStats {
            count: 1,
            p95_ms: 42_000,
        };
        let remote = PackageStats {
            count: 1,
            p95_ms: 24_000,
        };

        assert_eq!(paired_predictions(Some(&local), None), (42_000, 42_000));
        assert_eq!(paired_predictions(None, Some(&remote)), (24_000, 24_000));
        assert_eq!(
            paired_predictions(None, None),
            (DEFAULT_UNKNOWN_P95_MS, DEFAULT_UNKNOWN_P95_MS)
        );
    }

    #[test]
    fn active_local_builds_make_idle_remote_preferable() {
        let dir = test_data_dir("decision-active-local");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let observed_drv = "/nix/store/hash-kwin-6.6.3.drv";
        let active_drv = "/nix/store/hash-kwin-6.6.4.drv";
        record_event(&cfg, &build_event("start", observed_drv, 1_000, "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event("finish", observed_drv, 1_801_000, "success"),
        )
        .unwrap();
        record_event(&cfg, &build_event("start", active_drv, now_ms(), "unknown")).unwrap();
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-kwin-6.6.5.drv".to_string(),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "accept");
        let metrics = decision.metrics.unwrap();
        assert_eq!(metrics.local_samples, 1);
        assert_eq!(metrics.remote_samples, 0);
        assert_eq!(metrics.local_active_count, 1);
        assert!(metrics.local_completion_ms > metrics.remote_completion_ms);
        assert_eq!(remote_admission_count(&dir), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn exploration_can_refresh_unlucky_slow_local_sample() {
        let dir = test_data_dir("decision-explore-local");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        write_remote_stats(&dir, "kwin", 1, 10_000);
        let observed_drv = "/nix/store/hash-kwin-6.6.3.drv";
        record_event(&cfg, &build_event("start", observed_drv, 1_000, "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event("finish", observed_drv, 1_801_000, "success"),
        )
        .unwrap();
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: exploration_drv_path("kwin-refresh-local"),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "exploration: empty local host selected");
        let metrics = decision.metrics.unwrap();
        assert_eq!(metrics.local_samples, 1);
        assert_eq!(metrics.remote_samples, 1);
        assert!(metrics.remote_completion_ms < metrics.local_completion_ms);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn exploration_can_refresh_unlucky_slow_remote_sample() {
        let dir = test_data_dir("decision-explore-remote");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        write_remote_stats(&dir, "kwin", 1, 1_800_000);
        let observed_drv = "/nix/store/hash-kwin-6.6.3.drv";
        record_event(&cfg, &build_event("start", observed_drv, 1_000, "unknown")).unwrap();
        record_event(
            &cfg,
            &build_event("finish", observed_drv, 11_000, "success"),
        )
        .unwrap();
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: exploration_drv_path("kwin-refresh-remote"),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "accept");
        assert_eq!(decision.reason, "exploration: empty remote host selected");
        let metrics = decision.metrics.unwrap();
        assert_eq!(metrics.local_samples, 1);
        assert_eq!(metrics.remote_samples, 1);
        assert!(metrics.local_completion_ms < metrics.remote_completion_ms);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-stale");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, 1, 0.0);
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-kwin-6.6.3.drv".to_string(),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote telemetry is stale");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn busy_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-busy");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.95);
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-kwin-6.6.3.drv".to_string(),
            required_features: Vec::new(),
            pname: "kwin".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote cpu is busy");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn memory_pressure_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-memory-pressure");
        let cfg = test_config(dir.clone());
        write_remote_telemetry_full(&dir, now_ms(), 0.0, 64_000_000, 11.0, 0);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote memory pressure is high");
        assert_eq!(remote_admission_count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn low_memory_remote_telemetry_declines_decision() {
        let dir = test_data_dir("decision-low-memory");
        let cfg = test_config(dir.clone());
        write_remote_telemetry_full(&dir, now_ms(), 0.0, 1024 * 1024, 0.0, 0);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote memory is low");
        assert_eq!(remote_admission_count(&dir), 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn remote_admission_limit_declines_decision() {
        let dir = test_data_dir("decision-admission-limit");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let conn = open_history_db(&dir).unwrap();
        let admitted_at_ms = now_ms().saturating_sub(2_000);
        for idx in 0..DEFAULT_MAX_REMOTE_ADMITTED {
            insert_remote_admission(
                &conn,
                &format!("/nix/store/hash-admitted-{idx}.drv"),
                admitted_at_ms,
                10_000,
                false,
            );
        }
        drop(conn);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote admission limit reached");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_remote_admission_limit_declines_decision() {
        let dir = test_data_dir("decision-unknown-admission-limit");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let conn = open_history_db(&dir).unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-unknown-admitted.drv",
            now_ms().saturating_sub(2_000),
            10_000,
            true,
        );
        drop(conn);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "unknown remote admission limit reached");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_remote_admission_declines_decision() {
        let dir = test_data_dir("decision-admission-interval");
        let cfg = test_config(dir.clone());
        write_remote_telemetry(&dir, now_ms(), 0.0);
        let conn = open_history_db(&dir).unwrap();
        insert_remote_admission(
            &conn,
            "/nix/store/hash-recent-admitted.drv",
            now_ms(),
            10_000,
            false,
        );
        drop(conn);
        let candidate = test_candidate("/nix/store/hash-kwin-6.6.3.drv");

        let decision = decide_build_candidate(&cfg, &candidate).unwrap();
        assert_eq!(decision.decision, "decline");
        assert_eq!(decision.reason, "remote admission interval not elapsed");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn comparison_declines_when_local_prediction_is_faster() {
        let candidate = test_candidate(&non_exploration_drv_path("local-faster"));
        let outcome = compare_predictions(
            &candidate,
            &test_target(),
            test_metrics(10_000, 20_000),
            &SchedulerPolicy::default(),
        );

        assert!(!outcome.record_remote_admission);
        assert_eq!(outcome.decision.decision, "decline");
        assert_eq!(
            outcome.decision.reason,
            "local queue is predicted to drain sooner"
        );
    }

    #[test]
    fn comparison_accepts_when_remote_prediction_is_faster() {
        let candidate = test_candidate(&non_exploration_drv_path("remote-faster"));
        let outcome = compare_predictions(
            &candidate,
            &test_target(),
            test_metrics(20_000, 10_000),
            &SchedulerPolicy::default(),
        );

        assert!(outcome.record_remote_admission);
        assert_eq!(outcome.decision.decision, "accept");
        assert_eq!(
            outcome.decision.reason,
            "remote predicted 10000ms vs local 20000ms"
        );
        assert_eq!(
            outcome.decision.store_uri.as_deref(),
            Some(DEFAULT_REMOTE_STORE_URI)
        );
    }

    #[test]
    fn scheduler_decision_log_includes_candidate_and_reason() {
        let cfg = test_config(PathBuf::from("/tmp/unused"));
        let candidate = BuildCandidate {
            am_willing: 1,
            needed_system: "x86_64-linux".to_string(),
            drv_path: "/nix/store/hash-quoted\"package-1.0.drv".to_string(),
            required_features: vec!["kvm".to_string(), "big-parallel".to_string()],
            pname: "quoted\"package".to_string(),
            remote_host: DEFAULT_REMOTE_HOST.to_string(),
            remote_store_uri: DEFAULT_REMOTE_STORE_URI.to_string(),
        };
        let decision = Decision {
            decision: "decline".to_string(),
            reason: "remote cpu is busy".to_string(),
            store_uri: None,
            metrics: None,
        };

        let line = scheduler_decision_log_line(&cfg, &candidate, &decision);
        assert!(line.contains("scheduler_decision "));
        assert!(line.contains("remote_host=\"tsugumi\""));
        assert!(line.contains("decision=\"decline\""));
        assert!(line.contains("reason=\"remote cpu is busy\""));
        assert!(line.contains("required_features=\"kvm,big-parallel\""));
        assert!(line.contains("quoted\\\"package"));
        assert!(line.contains("store_uri=null"));
    }
}
