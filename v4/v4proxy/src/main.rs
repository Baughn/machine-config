use clap::Parser;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Command line arguments for the IPv4 to IPv6 Minecraft proxy
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(name = "minecraft-ipv6-proxy")]
#[command(about = "A reverse proxy between IPv4 internet and an IPv6-only Minecraft server")]
struct Args {
    /// List of ports to forward, format: local_port[:remote_port] (separated by commas)
    /// If only one port number is given, it's used for both local and remote
    #[arg(short, long, default_value = "25565,25566")]
    ports: String,

    /// Target IPv6 hostname
    #[arg(long, default_value = "direct.brage.info")]
    target: String,

    /// Connection timeout in seconds
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Buffer size for data transfer (in bytes)
    #[arg(long, default_value = "8192")]
    buffer_size: usize,
}

/// Represents a port mapping from a local IPv4 port to a remote IPv6 port
#[derive(Debug, Clone, PartialEq, Eq)]
struct PortMapping {
    /// The local IPv4 port to listen on
    local_port: u16,
    /// The remote IPv6 port to connect to
    remote_port: u16,
}

/// Parse a port mapping string in the format "local_port[:remote_port]"
fn parse_port_mapping(s: &str) -> Option<PortMapping> {
    if let Some(pos) = s.find(':') {
        let local_port = s[..pos].trim().parse().ok()?;
        let remote_port = s[pos + 1..].trim().parse().ok()?;
        Some(PortMapping {
            local_port,
            remote_port,
        })
    } else {
        let port = s.trim().parse().ok()?;
        Some(PortMapping {
            local_port: port,
            remote_port: port,
        })
    }
}

/// Copy data from a reader to a writer
///
/// Returns the number of bytes copied.
fn copy_data(mut reader: impl Read, mut writer: impl Write, buffer_size: usize) -> io::Result<u64> {
    let mut buffer = vec![0; buffer_size];
    let mut total = 0;

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // End of stream
            Ok(n) => {
                writer.write_all(&buffer[0..n])?;
                total += n as u64;
            }
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                // For non-blocking or timed out operations, retry after a short delay
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(total)
}

/// Handle an individual client connection
fn handle_client(
    client: TcpStream,
    target_host: &str,
    target_port: u16,
    timeout: Duration,
    buffer_size: usize,
) -> io::Result<()> {
    // Get client info for logging
    let client_addr = client.peer_addr()?;
    println!("Handling connection from {}", client_addr);

    // Connect to IPv6 target
    let target_addr = format!("{}:{}", target_host, target_port);
    match TcpStream::connect(&target_addr) {
        Ok(server) => {
            println!("Connected to target {}", target_addr);

            // Set read timeouts to avoid blocking indefinitely
            client.set_read_timeout(Some(timeout))?;
            server.set_read_timeout(Some(timeout))?;

            // Set write timeouts
            client.set_write_timeout(Some(timeout))?;
            server.set_write_timeout(Some(timeout))?;

            // Clone streams for reading and writing
            let client_read = client.try_clone()?;
            let server_read = server.try_clone()?;

            // Spawn a thread to forward data from client to server
            let client_to_server = {
                let target_addr = target_addr.clone();
                thread::spawn(move || match copy_data(client_read, server, buffer_size) {
                    Ok(bytes) => println!(
                        "{} → {}: {} bytes transferred",
                        client_addr, target_addr, bytes
                    ),
                    Err(e) => eprintln!(
                        "Error in {} → {} direction: {}",
                        client_addr, target_addr, e
                    ),
                })
            };

            // Forward data from server to client in the main thread
            match copy_data(server_read, client, buffer_size) {
                Ok(bytes) => println!(
                    "{} → {}: {} bytes transferred",
                    target_addr, client_addr, bytes
                ),
                Err(e) => eprintln!(
                    "Error in {} → {} direction: {}",
                    target_addr, client_addr, e
                ),
            }

            // Wait for the other thread to finish
            let _ = client_to_server.join();

            println!("Connection from {} closed", client_addr);
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to connect to target {}: {}", target_addr, e);
            Err(e)
        }
    }
}

/// Start a proxy server for a specific port mapping
fn start_proxy(
    mapping: PortMapping,
    target_host: &str,
    timeout: Duration,
    buffer_size: usize,
) -> io::Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", mapping.local_port))?;
    println!(
        "Listening on port {} and forwarding to [{}]:{}",
        mapping.local_port, target_host, mapping.remote_port
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let target = target_host.to_string();
                let remote_port = mapping.remote_port;
                let timeout_clone = timeout;
                let buffer_size_clone = buffer_size;

                thread::spawn(move || {
                    if let Err(e) = handle_client(
                        stream,
                        &target,
                        remote_port,
                        timeout_clone,
                        buffer_size_clone,
                    ) {
                        eprintln!("Error handling client: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
            }
        }
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let port_mappings: Vec<PortMapping> = args
        .ports
        .split(',')
        .filter_map(parse_port_mapping)
        .collect();

    if port_mappings.is_empty() {
        eprintln!("No valid ports specified");
        return Ok(());
    }

    let timeout = Duration::from_secs(args.timeout);
    let buffer_size = args.buffer_size;

    println!("Starting IPv4 to IPv6 proxy");
    println!("Target host: {}", args.target);
    println!("Timeout: {} seconds", args.timeout);
    println!("Buffer size: {} bytes", buffer_size);
    println!("Port mappings:");

    for mapping in &port_mappings {
        println!("  {} → {}", mapping.local_port, mapping.remote_port);
    }

    // Start a proxy for each port mapping
    let mut handles = vec![];
    let target_host = Arc::new(args.target);

    for mapping in port_mappings {
        let target = Arc::clone(&target_host);
        let handle = thread::spawn(move || {
            let local_port = mapping.local_port;
            if let Err(e) = start_proxy(mapping, &target, timeout, buffer_size) {
                eprintln!("Error starting proxy on local port {}: {}", local_port, e);
            }
        });
        handles.push(handle);
    }

    // Wait for all proxy threads
    for handle in handles {
        let _ = handle.join();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_port_mapping() {
        // Test single port
        let mapping = parse_port_mapping("25565").unwrap();
        assert_eq!(mapping.local_port, 25565);
        assert_eq!(mapping.remote_port, 25565);

        // Test port mapping
        let mapping = parse_port_mapping("8080:25565").unwrap();
        assert_eq!(mapping.local_port, 8080);
        assert_eq!(mapping.remote_port, 25565);

        // Test with spaces
        let mapping = parse_port_mapping(" 9000 : 8000 ").unwrap();
        assert_eq!(mapping.local_port, 9000);
        assert_eq!(mapping.remote_port, 8000);

        // Test invalid inputs
        assert!(parse_port_mapping("").is_none());
        assert!(parse_port_mapping("not_a_port").is_none());
        assert!(parse_port_mapping("8080:not_a_port").is_none());
        assert!(parse_port_mapping(":8080").is_none());
        assert!(parse_port_mapping("8080:").is_none());
    }

    #[test]
    fn test_copy_data() {
        use std::io::Cursor;

        let data = b"Hello, world!";
        let reader = Cursor::new(data);
        let mut writer = Vec::new();

        let bytes_copied = copy_data(reader, &mut writer, 1024).unwrap();

        assert_eq!(bytes_copied, data.len() as u64);
        assert_eq!(&writer, data);
    }

    #[test]
    fn test_port_mapping_equality() {
        let mapping1 = PortMapping {
            local_port: 8080,
            remote_port: 25565,
        };
        let mapping2 = PortMapping {
            local_port: 8080,
            remote_port: 25565,
        };
        let mapping3 = PortMapping {
            local_port: 9090,
            remote_port: 25565,
        };

        assert_eq!(mapping1, mapping2);
        assert_ne!(mapping1, mapping3);
    }
}
