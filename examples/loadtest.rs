//! SOCKS5 Proxy Load Testing Tool
//!
//! This tool performs load testing on a SOCKS5 proxy server with various scenarios:
//! - Concurrent connection tests (1000, 5000 connections)
//! - ACL performance testing
//! - Session tracking overhead
//! - Database write throughput
//!
//! Usage:
//!   cargo run --release --example loadtest -- --scenario <scenario> --proxy 127.0.0.1:1080
//!
//! Scenarios:
//!   - concurrent-1000: 1000 concurrent connections
//!   - concurrent-5000: 5000 concurrent connections
//!   - acl-perf: ACL evaluation performance test
//!   - session-overhead: Session tracking overhead test
//!   - db-throughput: Database write throughput test
//!   - all: Run all scenarios

use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, Clone, ValueEnum)]
enum Scenario {
    #[value(name = "concurrent-1000")]
    Concurrent1000,
    #[value(name = "concurrent-5000")]
    Concurrent5000,
    #[value(name = "acl-perf")]
    AclPerf,
    #[value(name = "session-overhead")]
    SessionOverhead,
    #[value(name = "db-throughput")]
    DbThroughput,
    All,
}

#[derive(Parser, Debug)]
#[command(name = "loadtest")]
#[command(about = "SOCKS5 Proxy Load Testing Tool", long_about = None)]
struct Args {
    /// Test scenario to run
    #[arg(short, long, value_enum)]
    scenario: Scenario,

    /// SOCKS5 proxy address
    #[arg(short, long, default_value = "127.0.0.1:1080")]
    proxy: SocketAddr,

    /// Upstream test server address (will use echo server)
    #[arg(short, long, default_value = "127.0.0.1:9999")]
    upstream: SocketAddr,

    /// Duration for sustained load tests (seconds)
    #[arg(short, long, default_value = "30")]
    duration: u64,

    /// Username for authenticated tests
    #[arg(short = 'U', long)]
    username: Option<String>,

    /// Password for authenticated tests
    #[arg(long)]
    password: Option<String>,

    /// Output results to JSON file
    #[arg(short, long)]
    output: Option<String>,
}

#[derive(Debug)]
struct TestMetrics {
    total_connections: AtomicUsize,
    successful_connections: AtomicUsize,
    failed_connections: AtomicUsize,
    total_duration_ns: AtomicU64,
    min_duration_ns: AtomicU64,
    max_duration_ns: AtomicU64,
    total_bytes_sent: AtomicU64,
    total_bytes_received: AtomicU64,
}

impl TestMetrics {
    fn new() -> Self {
        Self {
            total_connections: AtomicUsize::new(0),
            successful_connections: AtomicUsize::new(0),
            failed_connections: AtomicUsize::new(0),
            total_duration_ns: AtomicU64::new(0),
            min_duration_ns: AtomicU64::new(u64::MAX),
            max_duration_ns: AtomicU64::new(0),
            total_bytes_sent: AtomicU64::new(0),
            total_bytes_received: AtomicU64::new(0),
        }
    }

    fn record_success(&self, duration_ns: u64, bytes_sent: u64, bytes_received: u64) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.successful_connections.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ns.fetch_add(duration_ns, Ordering::Relaxed);
        self.total_bytes_sent.fetch_add(bytes_sent, Ordering::Relaxed);
        self.total_bytes_received
            .fetch_add(bytes_received, Ordering::Relaxed);

        // Update min
        self.min_duration_ns
            .fetch_min(duration_ns, Ordering::Relaxed);

        // Update max
        self.max_duration_ns
            .fetch_max(duration_ns, Ordering::Relaxed);
    }

    fn record_failure(&self) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.failed_connections.fetch_add(1, Ordering::Relaxed);
    }

    fn print_summary(&self, test_name: &str, elapsed: Duration) {
        let total = self.total_connections.load(Ordering::Relaxed);
        let successful = self.successful_connections.load(Ordering::Relaxed);
        let failed = self.failed_connections.load(Ordering::Relaxed);
        let total_dur = self.total_duration_ns.load(Ordering::Relaxed);
        let min_dur = self.min_duration_ns.load(Ordering::Relaxed);
        let max_dur = self.max_duration_ns.load(Ordering::Relaxed);
        let bytes_sent = self.total_bytes_sent.load(Ordering::Relaxed);
        let bytes_recv = self.total_bytes_received.load(Ordering::Relaxed);

        let avg_dur = if successful > 0 {
            total_dur / successful as u64
        } else {
            0
        };

        let success_rate = if total > 0 {
            (successful as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let throughput = if elapsed.as_secs() > 0 {
            successful as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        println!("\n{}", "=".repeat(80));
        println!("📊 Load Test Results: {}", test_name);
        println!("{}", "=".repeat(80));
        println!("\n⏱️  Test Duration: {:.2}s", elapsed.as_secs_f64());
        println!("\n📈 Connection Statistics:");
        println!("  Total Connections:      {}", total);
        println!("  ✅ Successful:          {} ({:.2}%)", successful, success_rate);
        println!("  ❌ Failed:              {}", failed);
        println!("  🔄 Throughput:          {:.2} conn/s", throughput);
        println!("\n⚡ Latency Statistics (SOCKS5 handshake):");
        println!("  Average:                {:.2} ms", avg_dur as f64 / 1_000_000.0);
        println!("  Minimum:                {:.2} ms", min_dur as f64 / 1_000_000.0);
        println!("  Maximum:                {:.2} ms", max_dur as f64 / 1_000_000.0);
        println!("\n📦 Data Transfer:");
        println!("  Bytes Sent:             {} ({:.2} MB)", bytes_sent, bytes_sent as f64 / 1_048_576.0);
        println!("  Bytes Received:         {} ({:.2} MB)", bytes_recv, bytes_recv as f64 / 1_048_576.0);
        println!("  Total Transfer:         {:.2} MB", (bytes_sent + bytes_recv) as f64 / 1_048_576.0);
        println!("{}", "=".repeat(80));
    }
}

/// Perform SOCKS5 handshake and CONNECT request
async fn socks5_connect(
    proxy_addr: SocketAddr,
    dest_addr: SocketAddr,
    username: Option<&str>,
    password: Option<&str>,
) -> std::io::Result<(TcpStream, Duration)> {
    let start = Instant::now();
    let mut stream = TcpStream::connect(proxy_addr).await?;

    // Step 1: Method negotiation
    let auth_method = if username.is_some() { 0x02 } else { 0x00 };
    stream.write_all(&[0x05, 0x01, auth_method]).await?;

    let mut response = [0u8; 2];
    stream.read_exact(&mut response).await?;

    if response[0] != 0x05 {
        return Err(std::io::Error::other("Invalid SOCKS version"));
    }

    // Step 2: Authentication (if required)
    if response[1] == 0x02 {
        let username = username.ok_or_else(|| std::io::Error::other("Username required"))?;
        let password = password.ok_or_else(|| std::io::Error::other("Password required"))?;

        let mut auth_req = Vec::new();
        auth_req.push(0x01); // Auth version
        auth_req.push(username.len() as u8);
        auth_req.extend_from_slice(username.as_bytes());
        auth_req.push(password.len() as u8);
        auth_req.extend_from_slice(password.as_bytes());
        stream.write_all(&auth_req).await?;

        let mut auth_resp = [0u8; 2];
        stream.read_exact(&mut auth_resp).await?;
        if auth_resp[1] != 0x00 {
            return Err(std::io::Error::other("Authentication failed"));
        }
    } else if response[1] != 0x00 {
        return Err(std::io::Error::other("Unsupported auth method"));
    }

    // Step 3: CONNECT request
    let mut request = Vec::new();
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x01]); // VER, CMD=CONNECT, RSV, ATYP=IPv4

    match dest_addr.ip() {
        std::net::IpAddr::V4(ip) => {
            request.extend_from_slice(&ip.octets());
        }
        std::net::IpAddr::V6(_) => {
            return Err(std::io::Error::other("IPv6 not supported in this test"));
        }
    }
    request.extend_from_slice(&dest_addr.port().to_be_bytes());
    stream.write_all(&request).await?;

    // Read response
    let mut reply = [0u8; 10];
    stream.read_exact(&mut reply).await?;

    if reply[1] != 0x00 {
        return Err(std::io::Error::other(format!(
            "SOCKS5 error: reply code {}",
            reply[1]
        )));
    }

    let elapsed = start.elapsed();
    Ok((stream, elapsed))
}

/// Test scenario: Concurrent connections
async fn test_concurrent_connections(
    args: &Args,
    count: usize,
    batch_size: usize,
) -> std::io::Result<()> {
    println!("\n🚀 Starting Concurrent Connections Test ({} connections)", count);
    println!("   Batch Size: {} connections", batch_size);
    println!("   Proxy: {}", args.proxy);

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();

    // Process in batches to avoid overwhelming the system
    for batch_start in (0..count).step_by(batch_size) {
        let batch_end = (batch_start + batch_size).min(count);
        let batch_count = batch_end - batch_start;

        let mut tasks = Vec::with_capacity(batch_count);

        for _ in 0..batch_count {
            let proxy_addr = args.proxy;
            let upstream_addr = args.upstream;
            let metrics = metrics.clone();
            let username = args.username.clone();
            let password = args.password.clone();

            tasks.push(tokio::spawn(async move {
                let result = timeout(
                    Duration::from_secs(10),
                    socks5_connect(
                        proxy_addr,
                        upstream_addr,
                        username.as_deref(),
                        password.as_deref(),
                    ),
                )
                .await;

                match result {
                    Ok(Ok((mut stream, duration))) => {
                        // Graceful shutdown to avoid CLOSE-WAIT
                        let _ = stream.shutdown().await;
                        drop(stream);
                        metrics.record_success(duration.as_nanos() as u64, 0, 0);
                        true
                    }
                    _ => {
                        metrics.record_failure();
                        false
                    }
                }
            }));
        }

        // Wait for batch to complete
        for task in tasks {
            let _ = task.await;
        }

        // Small delay between batches
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Progress indicator
        let completed = batch_end;
        let progress = (completed as f64 / count as f64) * 100.0;
        print!("\r   Progress: {}/{} ({:.1}%)", completed, count, progress);
        use std::io::Write;
        std::io::stdout().flush().unwrap();
    }

    println!(); // New line after progress

    let test_elapsed = test_start.elapsed();
    metrics.print_summary(
        &format!("{} Concurrent Connections", count),
        test_elapsed,
    );

    Ok(())
}

/// Test scenario: ACL performance
async fn test_acl_performance(args: &Args) -> std::io::Result<()> {
    println!("\n🔒 Starting ACL Performance Test");
    println!("   Duration: {} seconds", args.duration);
    println!("   Proxy: {}", args.proxy);

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Spawn multiple concurrent workers
    for _ in 0..100 {
        let proxy_addr = args.proxy;
        let upstream_addr = args.upstream;
        let metrics = metrics.clone();
        let username = args.username.clone();
        let password = args.password.clone();
        let end_time = test_start + duration;

        tasks.push(tokio::spawn(async move {
            while Instant::now() < end_time {
                let result = timeout(
                    Duration::from_secs(5),
                    socks5_connect(
                        proxy_addr,
                        upstream_addr,
                        username.as_deref(),
                        password.as_deref(),
                    ),
                )
                .await;

                match result {
                    Ok(Ok((mut stream, dur))) => {
                        let _ = stream.shutdown().await;
                        drop(stream);
                        metrics.record_success(dur.as_nanos() as u64, 0, 0);
                    }
                    _ => {
                        metrics.record_failure();
                    }
                }

                // Small delay between requests
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
    }

    // Wait for all workers to complete
    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("ACL Performance Test", test_elapsed);

    Ok(())
}

/// Test scenario: Session tracking overhead
async fn test_session_overhead(args: &Args) -> std::io::Result<()> {
    println!("\n📊 Starting Session Tracking Overhead Test");
    println!("   Duration: {} seconds", args.duration);
    println!("   Proxy: {}", args.proxy);

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Spawn workers that create sessions and send data
    for _ in 0..50 {
        let proxy_addr = args.proxy;
        let upstream_addr = args.upstream;
        let metrics = metrics.clone();
        let username = args.username.clone();
        let password = args.password.clone();
        let end_time = test_start + duration;

        tasks.push(tokio::spawn(async move {
            while Instant::now() < end_time {
                let result = timeout(
                    Duration::from_secs(5),
                    socks5_connect(
                        proxy_addr,
                        upstream_addr,
                        username.as_deref(),
                        password.as_deref(),
                    ),
                )
                .await;

                match result {
                    Ok(Ok((mut stream, dur))) => {
                        // Send some data to generate traffic
                        let test_data = b"TEST DATA FOR SESSION TRACKING";
                        let mut bytes_sent = 0u64;
                        let mut bytes_received = 0u64;

                        for _ in 0..10 {
                            if stream.write_all(test_data).await.is_ok() {
                                bytes_sent += test_data.len() as u64;

                                let mut buf = [0u8; 1024];
                                if let Ok(n) = stream.read(&mut buf).await {
                                    bytes_received += n as u64;
                                }
                            }
                        }

                        let _ = stream.shutdown().await;
                        drop(stream);
                        metrics.record_success(dur.as_nanos() as u64, bytes_sent, bytes_received);
                    }
                    _ => {
                        metrics.record_failure();
                    }
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }));
    }

    // Wait for all workers
    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("Session Tracking Overhead", test_elapsed);

    Ok(())
}

/// Test scenario: Database write throughput
async fn test_db_throughput(args: &Args) -> std::io::Result<()> {
    println!("\n💾 Starting Database Write Throughput Test");
    println!("   Duration: {} seconds", args.duration);
    println!("   Proxy: {}", args.proxy);

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Spawn many short-lived connections to stress database writes
    for _ in 0..200 {
        let proxy_addr = args.proxy;
        let upstream_addr = args.upstream;
        let metrics = metrics.clone();
        let username = args.username.clone();
        let password = args.password.clone();
        let end_time = test_start + duration;

        tasks.push(tokio::spawn(async move {
            while Instant::now() < end_time {
                let result = timeout(
                    Duration::from_secs(5),
                    socks5_connect(
                        proxy_addr,
                        upstream_addr,
                        username.as_deref(),
                        password.as_deref(),
                    ),
                )
                .await;

                match result {
                    Ok(Ok((mut stream, dur))) => {
                        // Immediately close to create session churn
                        let _ = stream.shutdown().await;
                        drop(stream);
                        metrics.record_success(dur.as_nanos() as u64, 0, 0);
                    }
                    _ => {
                        metrics.record_failure();
                    }
                }

                // Very short delay for high churn
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }));
    }

    // Wait for all workers
    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("Database Write Throughput", test_elapsed);

    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    println!("🔧 RustSocks Load Testing Tool");
    println!("{}", "=".repeat(80));
    println!("Configuration:");
    println!("  Proxy:      {}", args.proxy);
    println!("  Upstream:   {}", args.upstream);
    println!("  Scenario:   {:?}", args.scenario);
    if let Some(ref username) = args.username {
        println!("  Auth:       {} (authenticated)", username);
    }
    println!("{}", "=".repeat(80));

    match args.scenario {
        Scenario::Concurrent1000 => {
            test_concurrent_connections(&args, 1000, 50).await?;
        }
        Scenario::Concurrent5000 => {
            test_concurrent_connections(&args, 5000, 100).await?;
        }
        Scenario::AclPerf => {
            test_acl_performance(&args).await?;
        }
        Scenario::SessionOverhead => {
            test_session_overhead(&args).await?;
        }
        Scenario::DbThroughput => {
            test_db_throughput(&args).await?;
        }
        Scenario::All => {
            println!("\n🎯 Running All Test Scenarios\n");

            test_concurrent_connections(&args, 1000, 50).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            test_acl_performance(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            test_session_overhead(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            test_db_throughput(&args).await?;
        }
    }

    println!("\n✅ Load testing completed successfully!");

    Ok(())
}
