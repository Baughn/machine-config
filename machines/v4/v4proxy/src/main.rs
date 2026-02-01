use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::RwLock;

/// Command line arguments for the IPv4 to IPv6 proxy
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(name = "v4proxy")]
#[command(about = "A reverse proxy between IPv4 internet and IPv6 servers (TCP and UDP)")]
struct Args {
    /// Port mappings in format: protocol:local_port[:remote_port][@target] (comma-separated)
    /// Examples: tcp:25565, tcp:8080:80@web.example.com, udp:24454@voice.example.com
    #[arg(short, long)]
    mappings: String,

    /// Default target hostname (used when @target is omitted from mapping)
    #[arg(long, default_value = "direct.brage.info")]
    default_target: String,

    /// Connection timeout in seconds (for TCP)
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Buffer size for data transfer (in bytes)
    #[arg(long, default_value = "8192")]
    buffer_size: usize,

    /// UDP session timeout in seconds (inactive sessions are cleaned up)
    #[arg(long, default_value = "60")]
    udp_session_timeout: u64,
}

/// Protocol type for a port mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Tcp,
    Udp,
}

/// Represents a port mapping from a local IPv4 port to a remote IPv6 port
#[derive(Debug, Clone, PartialEq, Eq)]
struct PortMapping {
    protocol: Protocol,
    local_port: u16,
    remote_port: u16,
    target: String,
}

/// Parse a mapping string in the format "protocol:local_port[:remote_port][@target]"
fn parse_mapping(s: &str, default_target: &str) -> Option<PortMapping> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Split off target if present
    let (port_part, target) = if let Some(at_pos) = s.rfind('@') {
        let target = s[at_pos + 1..].trim();
        if target.is_empty() {
            return None;
        }
        (s[..at_pos].trim(), target.to_string())
    } else {
        (s, default_target.to_string())
    };

    // Parse protocol:ports
    let colon_pos = port_part.find(':')?;
    let protocol_str = port_part[..colon_pos].trim().to_lowercase();
    let protocol = match protocol_str.as_str() {
        "tcp" => Protocol::Tcp,
        "udp" => Protocol::Udp,
        _ => return None,
    };

    let ports_str = port_part[colon_pos + 1..].trim();

    // Parse local_port[:remote_port]
    let (local_port, remote_port) = if let Some(port_colon) = ports_str.find(':') {
        let local = ports_str[..port_colon].trim().parse().ok()?;
        let remote = ports_str[port_colon + 1..].trim().parse().ok()?;
        (local, remote)
    } else {
        let port = ports_str.parse().ok()?;
        (port, port)
    };

    Some(PortMapping {
        protocol,
        local_port,
        remote_port,
        target,
    })
}

/// Parse all mappings from command line
fn parse_mappings(mappings_str: &str, default_target: &str) -> Vec<PortMapping> {
    mappings_str
        .split(',')
        .filter_map(|s| parse_mapping(s, default_target))
        .collect()
}

/// Handle an individual TCP client connection
async fn handle_tcp_client(
    client: TcpStream,
    target_host: String,
    target_port: u16,
    timeout: Duration,
    buffer_size: usize,
) {
    let client_addr = match client.peer_addr() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("Failed to get client address: {}", e);
            return;
        }
    };
    println!("[TCP] Connection from {}", client_addr);

    // Connect to target with timeout
    let target_addr = format!("{}:{}", target_host, target_port);
    let server = match tokio::time::timeout(timeout, TcpStream::connect(&target_addr)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            eprintln!("[TCP] Failed to connect to {}: {}", target_addr, e);
            return;
        }
        Err(_) => {
            eprintln!("[TCP] Connection to {} timed out", target_addr);
            return;
        }
    };

    println!("[TCP] Connected to target {}", target_addr);

    // Disable Nagle's algorithm for lower latency
    let _ = server.set_nodelay(true);
    let _ = client.set_nodelay(true);

    let (mut client_read, mut client_write) = client.into_split();
    let (mut server_read, mut server_write) = server.into_split();

    let client_addr_clone = client_addr;
    let target_addr_clone = target_addr.clone();

    // Spawn task for client -> server direction
    let client_to_server = tokio::spawn(async move {
        let mut buffer = vec![0u8; buffer_size];
        let mut total: u64 = 0;
        loop {
            match client_read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    if server_write.write_all(&buffer[..n]).await.is_err() {
                        break;
                    }
                    total += n as u64;
                }
                Err(_) => break,
            }
        }
        println!(
            "[TCP] {} → {}: {} bytes",
            client_addr_clone, target_addr_clone, total
        );
    });

    // Server -> client direction in current task
    let mut buffer = vec![0u8; buffer_size];
    let mut total: u64 = 0;
    loop {
        match server_read.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                if client_write.write_all(&buffer[..n]).await.is_err() {
                    break;
                }
                total += n as u64;
            }
            Err(_) => break,
        }
    }
    println!("[TCP] {} → {}: {} bytes", target_addr, client_addr, total);

    let _ = client_to_server.await;
    println!("[TCP] Connection from {} closed", client_addr);
}

/// Start a TCP proxy server for a specific port mapping
async fn start_tcp_proxy(
    mapping: PortMapping,
    timeout: Duration,
    buffer_size: usize,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", mapping.local_port)).await?;
    println!(
        "[TCP] Listening on port {} → [{}]:{}",
        mapping.local_port, mapping.target, mapping.remote_port
    );

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let target = mapping.target.clone();
                let remote_port = mapping.remote_port;
                tokio::spawn(handle_tcp_client(
                    stream,
                    target,
                    remote_port,
                    timeout,
                    buffer_size,
                ));
            }
            Err(e) => {
                eprintln!("[TCP] Error accepting connection: {}", e);
            }
        }
    }
}

/// UDP session tracking
struct UdpSession {
    /// Socket connected to the upstream server
    upstream_socket: Arc<UdpSocket>,
    /// Last activity time
    last_activity: Instant,
}

/// Start a UDP proxy server for a specific port mapping
async fn start_udp_proxy(
    mapping: PortMapping,
    buffer_size: usize,
    session_timeout: Duration,
) -> std::io::Result<()> {
    let listener = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", mapping.local_port)).await?);
    println!(
        "[UDP] Listening on port {} → [{}]:{}",
        mapping.local_port, mapping.target, mapping.remote_port
    );

    let sessions: Arc<RwLock<HashMap<SocketAddr, UdpSession>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // Spawn session cleanup task
    let sessions_cleanup = Arc::clone(&sessions);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            let mut sessions = sessions_cleanup.write().await;
            let now = Instant::now();
            sessions.retain(|addr, session| {
                let expired = now.duration_since(session.last_activity) > session_timeout;
                if expired {
                    println!("[UDP] Session expired for {}", addr);
                }
                !expired
            });
        }
    });

    let target_addr = format!("{}:{}", mapping.target, mapping.remote_port);

    loop {
        let mut buffer = vec![0u8; buffer_size];
        match listener.recv_from(&mut buffer).await {
            Ok((len, client_addr)) => {
                let data = buffer[..len].to_vec();

                // Check if session exists
                let needs_new_session = {
                    let sessions_read = sessions.read().await;
                    !sessions_read.contains_key(&client_addr)
                };

                if needs_new_session {
                    // Create new session
                    match UdpSocket::bind("0.0.0.0:0").await {
                        Ok(upstream_socket) => {
                            // Connect the socket to the target
                            if let Err(e) = upstream_socket.connect(&target_addr).await {
                                eprintln!(
                                    "[UDP] Failed to connect to {} for client {}: {}",
                                    target_addr, client_addr, e
                                );
                                continue;
                            }

                            let upstream_socket = Arc::new(upstream_socket);
                            println!("[UDP] New session for {} → {}", client_addr, target_addr);

                            // Store session
                            {
                                let mut sessions_write = sessions.write().await;
                                sessions_write.insert(
                                    client_addr,
                                    UdpSession {
                                        upstream_socket: Arc::clone(&upstream_socket),
                                        last_activity: Instant::now(),
                                    },
                                );
                            }

                            // Spawn task to receive from upstream and forward to client
                            let listener_clone = Arc::clone(&listener);
                            let sessions_clone = Arc::clone(&sessions);
                            let upstream_clone = Arc::clone(&upstream_socket);
                            let buffer_size_clone = buffer_size;
                            let session_timeout_clone = session_timeout;
                            tokio::spawn(async move {
                                let mut buffer = vec![0u8; buffer_size_clone];
                                loop {
                                    match tokio::time::timeout(
                                        session_timeout_clone,
                                        upstream_clone.recv(&mut buffer),
                                    )
                                    .await
                                    {
                                        Ok(Ok(len)) => {
                                            if let Err(e) =
                                                listener_clone.send_to(&buffer[..len], client_addr).await
                                            {
                                                eprintln!(
                                                    "[UDP] Failed to send to client {}: {}",
                                                    client_addr, e
                                                );
                                                break;
                                            }
                                            // Update last activity
                                            if let Some(session) =
                                                sessions_clone.write().await.get_mut(&client_addr)
                                            {
                                                session.last_activity = Instant::now();
                                            }
                                        }
                                        Ok(Err(e)) => {
                                            eprintln!(
                                                "[UDP] Error receiving from upstream for {}: {}",
                                                client_addr, e
                                            );
                                            break;
                                        }
                                        Err(_) => {
                                            // Timeout - session will be cleaned up
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!(
                                "[UDP] Failed to create upstream socket for {}: {}",
                                client_addr, e
                            );
                            continue;
                        }
                    }
                }

                // Forward data to upstream
                let upstream_socket = {
                    let mut sessions_write = sessions.write().await;
                    if let Some(session) = sessions_write.get_mut(&client_addr) {
                        session.last_activity = Instant::now();
                        Some(Arc::clone(&session.upstream_socket))
                    } else {
                        None
                    }
                };

                if let Some(socket) = upstream_socket {
                    if let Err(e) = socket.send(&data).await {
                        eprintln!(
                            "[UDP] Failed to forward to upstream for {}: {}",
                            client_addr, e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("[UDP] Error receiving packet: {}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let mappings = parse_mappings(&args.mappings, &args.default_target);

    if mappings.is_empty() {
        eprintln!("No valid mappings specified");
        eprintln!("Format: protocol:local_port[:remote_port][@target]");
        eprintln!("Examples: tcp:25565, tcp:8080:80@web.example.com, udp:24454");
        return Ok(());
    }

    let timeout = Duration::from_secs(args.timeout);
    let buffer_size = args.buffer_size;
    let udp_session_timeout = Duration::from_secs(args.udp_session_timeout);

    println!("Starting IPv4 to IPv6 proxy");
    println!("Default target: {}", args.default_target);
    println!("TCP timeout: {} seconds", args.timeout);
    println!("UDP session timeout: {} seconds", args.udp_session_timeout);
    println!("Buffer size: {} bytes", buffer_size);
    println!("Mappings:");

    for mapping in &mappings {
        let proto = match mapping.protocol {
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
        };
        println!(
            "  [{}] {} → [{}]:{}",
            proto, mapping.local_port, mapping.target, mapping.remote_port
        );
    }

    // Start all proxy tasks
    let mut handles = vec![];

    for mapping in mappings {
        match mapping.protocol {
            Protocol::Tcp => {
                let handle = tokio::spawn(start_tcp_proxy(mapping, timeout, buffer_size));
                handles.push(handle);
            }
            Protocol::Udp => {
                let handle =
                    tokio::spawn(start_udp_proxy(mapping, buffer_size, udp_session_timeout));
                handles.push(handle);
            }
        }
    }

    // Wait for all proxy tasks (they run forever unless error)
    for handle in handles {
        if let Err(e) = handle.await {
            eprintln!("Proxy task failed: {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mapping_tcp_simple() {
        let mapping = parse_mapping("tcp:25565", "default.host").unwrap();
        assert_eq!(mapping.protocol, Protocol::Tcp);
        assert_eq!(mapping.local_port, 25565);
        assert_eq!(mapping.remote_port, 25565);
        assert_eq!(mapping.target, "default.host");
    }

    #[test]
    fn test_parse_mapping_tcp_with_remote_port() {
        let mapping = parse_mapping("tcp:8080:80", "default.host").unwrap();
        assert_eq!(mapping.protocol, Protocol::Tcp);
        assert_eq!(mapping.local_port, 8080);
        assert_eq!(mapping.remote_port, 80);
        assert_eq!(mapping.target, "default.host");
    }

    #[test]
    fn test_parse_mapping_tcp_with_target() {
        let mapping = parse_mapping("tcp:25565@custom.host", "default.host").unwrap();
        assert_eq!(mapping.protocol, Protocol::Tcp);
        assert_eq!(mapping.local_port, 25565);
        assert_eq!(mapping.remote_port, 25565);
        assert_eq!(mapping.target, "custom.host");
    }

    #[test]
    fn test_parse_mapping_tcp_full() {
        let mapping = parse_mapping("tcp:8080:80@web.example.com", "default.host").unwrap();
        assert_eq!(mapping.protocol, Protocol::Tcp);
        assert_eq!(mapping.local_port, 8080);
        assert_eq!(mapping.remote_port, 80);
        assert_eq!(mapping.target, "web.example.com");
    }

    #[test]
    fn test_parse_mapping_udp() {
        let mapping = parse_mapping("udp:24454@voice.example.com", "default.host").unwrap();
        assert_eq!(mapping.protocol, Protocol::Udp);
        assert_eq!(mapping.local_port, 24454);
        assert_eq!(mapping.remote_port, 24454);
        assert_eq!(mapping.target, "voice.example.com");
    }

    #[test]
    fn test_parse_mapping_with_spaces() {
        let mapping = parse_mapping(" tcp : 9000 : 8000 @ host.com ", "default.host").unwrap();
        assert_eq!(mapping.protocol, Protocol::Tcp);
        assert_eq!(mapping.local_port, 9000);
        assert_eq!(mapping.remote_port, 8000);
        assert_eq!(mapping.target, "host.com");
    }

    #[test]
    fn test_parse_mapping_invalid() {
        assert!(parse_mapping("", "default").is_none());
        assert!(parse_mapping("invalid", "default").is_none());
        assert!(parse_mapping("25565", "default").is_none()); // Missing protocol
        assert!(parse_mapping("tcp:", "default").is_none()); // Missing port
        assert!(parse_mapping("tcp:abc", "default").is_none()); // Invalid port
        assert!(parse_mapping("ftp:25565", "default").is_none()); // Invalid protocol
        assert!(parse_mapping("tcp:25565@", "default").is_none()); // Empty target
    }

    #[test]
    fn test_parse_mappings() {
        let mappings = parse_mappings(
            "tcp:25565,tcp:25566,udp:24454@voice.host",
            "default.host",
        );
        assert_eq!(mappings.len(), 3);

        assert_eq!(mappings[0].protocol, Protocol::Tcp);
        assert_eq!(mappings[0].local_port, 25565);
        assert_eq!(mappings[0].target, "default.host");

        assert_eq!(mappings[1].protocol, Protocol::Tcp);
        assert_eq!(mappings[1].local_port, 25566);
        assert_eq!(mappings[1].target, "default.host");

        assert_eq!(mappings[2].protocol, Protocol::Udp);
        assert_eq!(mappings[2].local_port, 24454);
        assert_eq!(mappings[2].target, "voice.host");
    }

    #[test]
    fn test_port_mapping_equality() {
        let mapping1 = PortMapping {
            protocol: Protocol::Tcp,
            local_port: 8080,
            remote_port: 80,
            target: "host.com".to_string(),
        };
        let mapping2 = PortMapping {
            protocol: Protocol::Tcp,
            local_port: 8080,
            remote_port: 80,
            target: "host.com".to_string(),
        };
        let mapping3 = PortMapping {
            protocol: Protocol::Udp,
            local_port: 8080,
            remote_port: 80,
            target: "host.com".to_string(),
        };

        assert_eq!(mapping1, mapping2);
        assert_ne!(mapping1, mapping3);
    }
}
