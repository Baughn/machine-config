use clap::Parser;
use std::fs;
use std::io::{self, Read};
use std::net::{ToSocketAddrs, UdpSocket};
use std::path::PathBuf;

const MAGIC_PACKET_SIZE: usize = 64;
const DEFAULT_PORT: u16 = 999;

#[derive(Parser, Debug)]
#[command(name = "magic-reboot-send")]
#[command(about = "Send magic packet to trigger emergency reboot on remote machine")]
#[command(version)]
struct Args {
    /// Target hostname or IP address
    target: String,

    /// Target port
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Path to the 64-byte magic key file (use '-' for stdin)
    #[arg(short, long)]
    key: PathBuf,

    /// Don't actually send, just verify the key file
    #[arg(long)]
    dry_run: bool,
}

fn read_key(path: &PathBuf) -> io::Result<Vec<u8>> {
    if path.to_str() == Some("-") {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer)?;
        Ok(buffer)
    } else {
        fs::read(path)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Read the magic key
    let key = read_key(&args.key)?;

    if key.len() != MAGIC_PACKET_SIZE {
        return Err(format!(
            "Key file must be exactly {} bytes, got {} bytes",
            MAGIC_PACKET_SIZE,
            key.len()
        )
        .into());
    }

    if args.dry_run {
        println!("Dry run mode - key file validated ({} bytes)", key.len());
        println!("Would send to {}:{}", args.target, args.port);
        return Ok(());
    }

    // Resolve target address
    let target = format!("{}:{}", args.target, args.port);
    let addr = target
        .to_socket_addrs()?
        .next()
        .ok_or("Failed to resolve target address")?;

    // Create UDP socket matching the address family
    let bind_addr = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let socket = UdpSocket::bind(bind_addr)?;

    // Send the magic packet
    let bytes_sent = socket.send_to(&key, addr)?;

    println!(
        "Sent {} byte magic packet to {}",
        bytes_sent, addr
    );
    println!("If the target machine has magic-reboot loaded, it should reboot now.");

    Ok(())
}
