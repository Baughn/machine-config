use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_UNKNOWN_P95_MS: u64 = 30 * 60 * 1000;
const DEFAULT_STALE_START_MS: u128 = 24 * 60 * 60 * 1000;
const DEFAULT_MAX_SAMPLES_PER_PNAME: usize = 200;
const SCHEMA_VERSION: i64 = 1;

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

#[derive(Clone, Copy, Debug)]
enum Mode {
    Agent,
    Controller,
}

#[derive(Debug)]
struct Telemetry {
    host: String,
    timestamp_ms: u128,
    cpu_busy_ratio: Option<f64>,
    mem_total_kb: Option<u64>,
    mem_available_kb: Option<u64>,
    psi_memory_some_avg10: Option<f64>,
    nix_slots_total: usize,
    nix_slots_local: usize,
    nix_slots_remote: usize,
}

#[derive(Debug)]
struct BuildEvent {
    kind: String,
    drv_path: String,
    out_paths: String,
    status: String,
    host: String,
    timestamp_ms: u128,
}

fn main() {
    if let Err(err) = real_main() {
        eprintln!("nix-build-balancer: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> io::Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("serve") => serve(parse_serve_args(args.collect())?),
        Some("event") => send_event(parse_event_args(args.collect())?),
        Some("telemetry") => {
            let cfg = parse_serve_args(args.collect())?;
            println!("{}", telemetry_json(&read_telemetry(&cfg.host)?));
            Ok(())
        }
        _ => {
            print_usage();
            Ok(())
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage:
  nix-build-balancer serve --mode agent|controller --host NAME --data-dir DIR [--unix-socket PATH] [--listen ADDR:PORT] [--remote NAME=ADDR:PORT]
  nix-build-balancer event --endpoint unix:PATH|tcp:ADDR:PORT --kind start|finish --host NAME --drv-path PATH [--out-paths PATHS] [--status STATUS]"
    );
}

fn parse_serve_args(args: Vec<String>) -> io::Result<Config> {
    let mut cfg = Config {
        mode: Mode::Agent,
        host: hostname_fallback(),
        data_dir: PathBuf::from("/var/lib/nix-build-balancer"),
        unix_socket: None,
        listen: None,
        remote: Vec::new(),
        poll_interval: Duration::from_secs(1),
        max_samples_per_pname: DEFAULT_MAX_SAMPLES_PER_PNAME,
        stale_start_ms: DEFAULT_STALE_START_MS,
        once: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                cfg.mode = match args.get(i).map(String::as_str) {
                    Some("agent") => Mode::Agent,
                    Some("controller") => Mode::Controller,
                    other => invalid(format!("invalid mode {other:?}"))?,
                };
            }
            "--host" => {
                i += 1;
                cfg.host = required(&args, i, "--host")?.to_string();
            }
            "--data-dir" => {
                i += 1;
                cfg.data_dir = PathBuf::from(required(&args, i, "--data-dir")?);
            }
            "--unix-socket" => {
                i += 1;
                cfg.unix_socket = Some(PathBuf::from(required(&args, i, "--unix-socket")?));
            }
            "--listen" => {
                i += 1;
                cfg.listen = Some(required(&args, i, "--listen")?.to_string());
            }
            "--remote" => {
                i += 1;
                cfg.remote.push(required(&args, i, "--remote")?.to_string());
            }
            "--poll-interval-ms" => {
                i += 1;
                let value = required(&args, i, "--poll-interval-ms")?
                    .parse::<u64>()
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
                cfg.poll_interval = Duration::from_millis(value);
            }
            "--max-samples-per-pname" => {
                i += 1;
                cfg.max_samples_per_pname = required(&args, i, "--max-samples-per-pname")?
                    .parse::<usize>()
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
            }
            "--stale-start-ms" => {
                i += 1;
                cfg.stale_start_ms = required(&args, i, "--stale-start-ms")?
                    .parse::<u128>()
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
            }
            "--once" => cfg.once = true,
            other => invalid(format!("unknown argument {other}"))?,
        }
        i += 1;
    }

    if cfg.unix_socket.is_none() && cfg.listen.is_none() && !cfg.once {
        return invalid("serve needs --unix-socket or --listen");
    }

    Ok(cfg)
}

fn parse_event_args(args: Vec<String>) -> io::Result<(String, BuildEvent)> {
    let mut endpoint = None;
    let mut kind = None;
    let mut drv_path = None;
    let mut out_paths = String::new();
    let mut status = "unknown".to_string();
    let mut host = hostname_fallback();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--endpoint" => {
                i += 1;
                endpoint = Some(required(&args, i, "--endpoint")?.to_string());
            }
            "--kind" => {
                i += 1;
                kind = Some(required(&args, i, "--kind")?.to_string());
            }
            "--drv-path" => {
                i += 1;
                drv_path = Some(required(&args, i, "--drv-path")?.to_string());
            }
            "--out-paths" => {
                i += 1;
                out_paths = required(&args, i, "--out-paths")?.to_string();
            }
            "--status" => {
                i += 1;
                status = required(&args, i, "--status")?.to_string();
            }
            "--host" => {
                i += 1;
                host = required(&args, i, "--host")?.to_string();
            }
            other => invalid(format!("unknown argument {other}"))?,
        }
        i += 1;
    }

    let event = BuildEvent {
        kind: kind
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--kind is required"))?,
        drv_path: drv_path
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--drv-path is required"))?,
        out_paths,
        status,
        host,
        timestamp_ms: now_ms(),
    };
    Ok((
        endpoint
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--endpoint is required"))?,
        event,
    ))
}

fn required<'a>(args: &'a [String], i: usize, name: &str) -> io::Result<&'a str> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} needs a value")))
}

fn invalid<T>(message: impl Into<String>) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
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
        ("POST", "/decision/build-candidate") => {
            let body =
                "{\"decision\":\"decline\",\"reason\":\"scheduler phase not implemented\"}\n";
            write_response(&mut stream, 200, "application/json", body)
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

fn drain_response<R: Read>(mut stream: R) -> io::Result<()> {
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.starts_with("HTTP/1.1 200 ") {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, response))
    }
}

fn event_body(event: &BuildEvent) -> String {
    format!(
        "kind={}\ndrv_path={}\nout_paths={}\nstatus={}\nhost={}\ntimestamp_ms={}\n",
        encode_value(&event.kind),
        encode_value(&event.drv_path),
        encode_value(&event.out_paths),
        encode_value(&event.status),
        encode_value(&event.host),
        event.timestamp_ms
    )
}

fn parse_event_body(body: &[u8]) -> io::Result<BuildEvent> {
    let text = String::from_utf8_lossy(body);
    let mut map = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.to_string(), decode_value(value));
        }
    }
    Ok(BuildEvent {
        kind: map
            .get("kind")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        drv_path: map.get("drv_path").cloned().unwrap_or_default(),
        out_paths: map.get("out_paths").cloned().unwrap_or_default(),
        status: map
            .get("status")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string()),
        host: map.get("host").cloned().unwrap_or_else(hostname_fallback),
        timestamp_ms: map
            .get("timestamp_ms")
            .and_then(|value| value.parse::<u128>().ok())
            .unwrap_or_else(now_ms),
    })
}

fn encode_value(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            b'\n' => out.push_str("%0A"),
            b'\r' => out.push_str("%0D"),
            b'=' => out.push_str("%3D"),
            b'\t' => out.push_str("%09"),
            _ => out.push(byte as char),
        }
    }
    out
}

fn decode_value(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(hex);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
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

fn cleanup_state(cfg: &Config) -> io::Result<()> {
    let conn = open_history_db(&cfg.data_dir)?;
    cleanup_stale_starts(&conn, cfg.stale_start_ms)
}

fn open_history_db(data_dir: &Path) -> io::Result<Connection> {
    fs::create_dir_all(data_dir)?;
    let conn = Connection::open(data_dir.join("history.sqlite3")).map_err(sqlite_error)?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(sqlite_error)?;
    init_history_schema(&conn)?;
    Ok(conn)
}

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
    let stat = fs::read_to_string("/proc/stat")?;
    let line = stat
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing aggregate cpu line"))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect();
    let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
    let total = values.iter().sum();
    Ok(CpuSample { idle, total })
}

fn read_meminfo() -> io::Result<(Option<u64>, Option<u64>)> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    let mut total = None;
    let mut available = None;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
        } else if line.starts_with("MemAvailable:") {
            available = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
        }
    }
    Ok((total, available))
}

fn read_psi_memory_some_avg10() -> io::Result<Option<f64>> {
    let psi = fs::read_to_string("/proc/pressure/memory")?;
    for line in psi.lines() {
        if let Some(rest) = line.strip_prefix("some ") {
            for field in rest.split_whitespace() {
                if let Some(value) = field.strip_prefix("avg10=") {
                    return Ok(value.parse::<f64>().ok());
                }
            }
        }
    }
    Ok(None)
}

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
        total += 1;
        if name.contains("localhost") {
            local += 1;
        } else if name.starts_with("ssh") {
            remote += 1;
        }
    }
    (total, local, remote)
}

fn telemetry_json(telemetry: &Telemetry) -> String {
    format!(
        "{{\"host\":\"{}\",\"timestamp_ms\":{},\"cpu_busy_ratio\":{},\"mem_total_kb\":{},\"mem_available_kb\":{},\"psi_memory_some_avg10\":{},\"nix_slots_total\":{},\"nix_slots_local\":{},\"nix_slots_remote\":{}}}\n",
        json_escape(&telemetry.host),
        telemetry.timestamp_ms,
        json_opt_f64(telemetry.cpu_busy_ratio),
        json_opt_u64(telemetry.mem_total_kb),
        json_opt_u64(telemetry.mem_available_kb),
        json_opt_f64(telemetry.psi_memory_some_avg10),
        telemetry.nix_slots_total,
        telemetry.nix_slots_local,
        telemetry.nix_slots_remote
    )
}

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

    let mut entries = Vec::new();
    for (pname, mut values) in durations {
        values.sort_unstable();
        let count = values.len();
        entries.push(format!(
            "{{\"pname\":\"{}\",\"count\":{},\"p50_ms\":{},\"p80_ms\":{},\"p95_ms\":{}}}",
            json_escape(&pname),
            count,
            quantile(&values, 0.50).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
            quantile(&values, 0.80).unwrap_or(DEFAULT_UNKNOWN_P95_MS),
            quantile(&values, 0.95).unwrap_or(DEFAULT_UNKNOWN_P95_MS)
        ));
    }

    Ok(format!(
        "{{\"unknown_p95_ms\":{},\"packages\":[{}]}}\n",
        DEFAULT_UNKNOWN_P95_MS,
        entries.join(",")
    ))
}

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
    io::Error::new(io::ErrorKind::Other, err)
}

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
        Err(io::Error::new(io::ErrorKind::Other, response))
    }
}

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

fn looks_versionish(part: &str) -> bool {
    part.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || part
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '.' || ch == '_' || ch == '+')
}

fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn json_opt_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.4}"))
        .unwrap_or_else(|| "null".to_string())
}

fn json_opt_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
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
}
