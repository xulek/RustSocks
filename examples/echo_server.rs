//! Simple Echo Server for Load Testing
//!
//! This server echoes back any data it receives, useful for testing
//! SOCKS5 proxy data transfer functionality.
//!
//! Usage:
//!   cargo run --release --example echo_server -- --port 9999

use clap::Parser;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Parser, Debug)]
#[command(name = "echo_server")]
#[command(about = "Simple TCP Echo Server for testing", long_about = None)]
struct Args {
    /// Bind address
    #[arg(short, long, default_value = "127.0.0.1")]
    bind: String,

    /// Bind port
    #[arg(short, long, default_value = "9999")]
    port: u16,
}

async fn handle_client(mut stream: TcpStream, client_addr: SocketAddr) {
    // Optimize TCP socket for low-latency echo
    let _ = stream.set_nodelay(true); // Disable Nagle's algorithm

    // Increase buffer sizes for better throughput
    let sock_ref = socket2::SockRef::from(&stream);
    let _ = sock_ref.set_recv_buffer_size(262144); // 256 KB
    let _ = sock_ref.set_send_buffer_size(262144); // 256 KB

    let mut buf = vec![0u8; 65536]; // Increased buffer size from 8KB to 64KB

    loop {
        match stream.read(&mut buf).await {
            Ok(0) => {
                // Connection closed
                break;
            }
            Ok(n) => {
                // Echo back the data
                if stream.write_all(&buf[..n]).await.is_err() {
                    eprintln!("Error writing to {}", client_addr);
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error reading from {}: {}", client_addr, e);
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let bind_addr = format!("{}:{}", args.bind, args.port);

    // Create socket with SO_REUSEADDR and SO_REUSEPORT for better performance
    let addr: std::net::SocketAddr = bind_addr
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let socket = socket2::Socket::new(
        if addr.is_ipv4() {
            socket2::Domain::IPV4
        } else {
            socket2::Domain::IPV6
        },
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;

    socket.set_reuse_address(true)?;
    socket.set_recv_buffer_size(262144)?; // 256 KB
    socket.set_send_buffer_size(262144)?; // 256 KB
    socket.set_nodelay(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?; // Increased backlog

    let listener = TcpListener::from_std(socket.into())?;
    println!("🔊 Echo server listening on {}", bind_addr);
    println!("   TCP optimizations: SO_REUSEADDR, TCP_NODELAY, 256KB buffers, 64KB read buffer");

    loop {
        match listener.accept().await {
            Ok((stream, client_addr)) => {
                tokio::spawn(async move {
                    handle_client(stream, client_addr).await;
                });
            }
            Err(e) => {
                eprintln!("Error accepting connection: {}", e);
            }
        }
    }
}
