use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_UNKNOWN_P95_MS: u64 = 30 * 60 * 1000;

#[derive(Clone, Debug)]
struct Config {
    mode: Mode,
    host: String,
    data_dir: PathBuf,
    unix_socket: Option<PathBuf>,
    listen: Option<String>,
    remote: Vec<String>,
    poll_interval: Duration,
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
    fs::create_dir_all(cfg.data_dir.join("starts"))?;

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
            record_event(&cfg.data_dir, &event)?;
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

fn record_event(data_dir: &Path, event: &BuildEvent) -> io::Result<()> {
    fs::create_dir_all(data_dir)?;
    fs::create_dir_all(data_dir.join("starts"))?;
    append_line(
        &data_dir.join("events.tsv"),
        &format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            event.timestamp_ms,
            event.host,
            event.kind,
            pname_from_drv(&event.drv_path),
            event.status,
            event.drv_path,
            event.out_paths
        ),
    )?;

    let start_file = data_dir
        .join("starts")
        .join(escape_filename(&event.drv_path));
    if event.kind == "start" {
        fs::write(start_file, event.timestamp_ms.to_string())?;
    } else if event.kind == "finish" {
        let start_ms = fs::read_to_string(&start_file)
            .ok()
            .and_then(|value| value.trim().parse::<u128>().ok());
        let _ = fs::remove_file(&start_file);
        if let Some(start_ms) = start_ms {
            let duration_ms = event.timestamp_ms.saturating_sub(start_ms);
            append_line(
                &data_dir.join("builds.tsv"),
                &format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    event.timestamp_ms,
                    event.host,
                    pname_from_drv(&event.drv_path),
                    duration_ms,
                    event.status,
                    event.drv_path,
                    event.out_paths
                ),
            )?;
        }
    }
    Ok(())
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
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
    let builds = data_dir.join("builds.tsv");
    let mut durations: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    if builds.exists() {
        let file = File::open(builds)?;
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() >= 4 {
                if let Ok(duration) = fields[3].parse::<u64>() {
                    durations
                        .entry(fields[2].to_string())
                        .or_default()
                        .push(duration);
                }
            }
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

fn escape_filename(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02x}"),
        })
        .collect()
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
}
