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
    let mut buf = vec![0u8; 8192];

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

    let listener = TcpListener::bind(&bind_addr).await?;
    println!("🔊 Echo server listening on {}", bind_addr);

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
