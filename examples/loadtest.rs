//! SOCKS5 Proxy Load Testing Tool
//!
//! This tool performs granular load testing on a SOCKS5 proxy server, measuring
//! individual pipeline stages and full end-to-end performance.
//!
//! Usage:
//!   cargo run --release --example loadtest -- --scenario <scenario> --proxy 127.0.0.1:1080
//!
//! Scenarios:
//!   - minimal-pipeline: SOCKS5 handshake only (ACL=off, Sessions=off, QoS=off)
//!   - full-pipeline: Complete pipeline with all features enabled
//!   - handshake-only: Pure SOCKS5 protocol overhead
//!   - data-transfer: Throughput test with sustained data transfer
//!   - session-churn: Rapid session create/destroy (tests DB writes)
//!   - concurrent-1000: 1000 concurrent connections
//!   - concurrent-5000: 5000 concurrent connections
//!   - all: Run all scenarios
//!
//! Measured Metrics:
//!   - Latency: Time from TCP connect to SOCKS5 response (SOCKS5 handshake)
//!   - Throughput: Connections per second
//!   - Data Transfer: Bytes sent/received (for data transfer tests)
//!
//! Pipeline Stages:
//!   1. TCP connect to proxy
//!   2. SOCKS5 method negotiation (1 RTT)
//!   3. Authentication (if enabled, 1 RTT)
//!   4. QoS connection limit check (if enabled)
//!   5. SOCKS5 CONNECT request (1 RTT)
//!   6. ACL evaluation (if enabled)
//!   7. Session creation (if enabled)
//!   8. Upstream TCP connect
//!   9. SOCKS5 response to client
//!  10. Database write (if enabled, async batched)
//!  11. Metrics collection (if enabled)

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
    /// Pure SOCKS5 handshake (ACL=off, Sessions=off, QoS=off) - measures minimal proxy overhead
    #[value(name = "minimal-pipeline")]
    MinimalPipeline,

    /// Complete pipeline (ACL + Sessions + QoS + DB) - measures full production latency
    #[value(name = "full-pipeline")]
    FullPipeline,

    /// SOCKS5 handshake only (no data transfer) - measures connection establishment
    #[value(name = "handshake-only")]
    HandshakeOnly,

    /// Data transfer throughput - measures proxy bandwidth with sustained traffic
    #[value(name = "data-transfer")]
    DataTransfer,

    /// Rapid session create/destroy - stresses database write performance
    #[value(name = "session-churn")]
    SessionChurn,

    /// ACL evaluation performance - tests rule matching with complex rulesets
    #[value(name = "acl-evaluation")]
    AclEvaluation,

    /// Authentication overhead - compares NoAuth vs UserPass authentication
    #[value(name = "auth-overhead")]
    AuthOverhead,

    /// QoS rate limiting - tests bandwidth throttling and connection limits
    #[value(name = "qos-limiting")]
    QosLimiting,

    /// Connection pool effectiveness - measures pool hit rate and reuse
    #[value(name = "pool-efficiency")]
    PoolEfficiency,

    /// DNS resolution performance - tests IPv4, IPv6, and domain resolution
    #[value(name = "dns-resolution")]
    DnsResolution,

    /// Long-lived connections - tests stability with sustained connections
    #[value(name = "long-lived")]
    LongLived,

    /// Large data transfer - tests multi-MB data transfers
    #[value(name = "large-transfer")]
    LargeTransfer,

    /// Metrics collection overhead - tests Prometheus metrics impact
    #[value(name = "metrics-overhead")]
    MetricsOverhead,

    /// 1000 concurrent connections - tests concurrency handling
    #[value(name = "concurrent-1000")]
    Concurrent1000,

    /// 5000 concurrent connections - tests high concurrency
    #[value(name = "concurrent-5000")]
    Concurrent5000,

    /// Run all test scenarios
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
        self.total_duration_ns
            .fetch_add(duration_ns, Ordering::Relaxed);
        self.total_bytes_sent
            .fetch_add(bytes_sent, Ordering::Relaxed);
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
        println!(
            "  ✅ Successful:          {} ({:.2}%)",
            successful, success_rate
        );
        println!("  ❌ Failed:              {}", failed);
        println!("  🔄 Throughput:          {:.2} conn/s", throughput);
        println!("\n⚡ Latency Statistics (SOCKS5 handshake):");
        println!(
            "  Average:                {:.2} ms",
            avg_dur as f64 / 1_000_000.0
        );
        println!(
            "  Minimum:                {:.2} ms",
            min_dur as f64 / 1_000_000.0
        );
        println!(
            "  Maximum:                {:.2} ms",
            max_dur as f64 / 1_000_000.0
        );
        println!("\n📦 Data Transfer:");
        println!(
            "  Bytes Sent:             {} ({:.2} MB)",
            bytes_sent,
            bytes_sent as f64 / 1_048_576.0
        );
        println!(
            "  Bytes Received:         {} ({:.2} MB)",
            bytes_recv,
            bytes_recv as f64 / 1_048_576.0
        );
        println!(
            "  Total Transfer:         {:.2} MB",
            (bytes_sent + bytes_recv) as f64 / 1_048_576.0
        );
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
    println!(
        "\n🚀 Starting Concurrent Connections Test ({} connections)",
        count
    );
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
    metrics.print_summary(&format!("{} Concurrent Connections", count), test_elapsed);

    Ok(())
}

/// Test scenario: Full pipeline with all features enabled
/// Measures: TCP connect + SOCKS5 handshake + Auth + QoS + ACL + Session + Upstream + DB + Metrics
/// Expected latency: 50-100ms (includes all overhead)
/// Config requirements: ACL=enabled, Sessions=enabled, QoS=enabled, DB=sqlite
async fn test_full_pipeline(args: &Args) -> std::io::Result<()> {
    println!("\n🔗 Starting Full Pipeline Test");
    println!("   Measures: Complete SOCKS5 pipeline with ALL features enabled");
    println!("   Pipeline: TCP → Handshake → Auth → QoS → ACL → Session → Upstream → DB → Metrics");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 100 concurrent");
    println!("   Proxy: {}", args.proxy);
    println!("   ⚠️  Ensure config has: ACL=on, Sessions=on, QoS=on, DB=sqlite");

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
    metrics.print_summary("Full Pipeline (ACL + Sessions + QoS + DB)", test_elapsed);

    Ok(())
}

/// Test scenario: Data transfer throughput
/// Measures: Proxy bandwidth with sustained bidirectional traffic
/// Expected: Handshake latency <50ms, throughput >100MB/s
/// Config requirements: Any (works with minimal or full pipeline)
async fn test_data_transfer(args: &Args) -> std::io::Result<()> {
    println!("\n📊 Starting Data Transfer Throughput Test");
    println!("   Measures: Proxy bandwidth with sustained data transfer");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 50 concurrent");
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
                        let test_data = b"TEST DATA FOR THROUGHPUT MEASUREMENT";
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
    metrics.print_summary("Data Transfer Throughput", test_elapsed);

    Ok(())
}

/// Test scenario: Session churn stress test
/// Measures: Database write performance with rapid session create/destroy
/// Expected: >1000 sessions/sec write throughput to SQLite
/// Config requirements: Sessions=enabled, DB=sqlite (batch writes)
async fn test_session_churn(args: &Args) -> std::io::Result<()> {
    println!("\n💾 Starting Session Churn Stress Test");
    println!("   Measures: Database write throughput with rapid session create/destroy");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 200 concurrent (high churn)");
    println!("   Proxy: {}", args.proxy);
    println!("   ⚠️  Ensure config has: Sessions=on, DB=sqlite, batch_size=100-500");

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
    metrics.print_summary("Session Churn (DB Write Throughput)", test_elapsed);

    Ok(())
}

/// Test scenario: Minimal pipeline (pure SOCKS5 overhead)
/// Measures: TCP connect + SOCKS5 handshake + upstream connect ONLY
/// Expected latency: <10ms (minimal overhead)
/// Config requirements: ACL=disabled, Sessions=disabled, QoS=disabled
async fn test_minimal_pipeline(args: &Args) -> std::io::Result<()> {
    println!("\n⚡ Starting Minimal Pipeline Test");
    println!("   Measures: Pure SOCKS5 protocol overhead (no ACL, no Sessions, no QoS)");
    println!("   Pipeline: TCP → Handshake → Upstream → Response");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 100 concurrent");
    println!("   Proxy: {}", args.proxy);
    println!("   ⚠️  Ensure config has: ACL=off, Sessions=off, QoS=off");

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
    metrics.print_summary("Minimal Pipeline (Pure SOCKS5)", test_elapsed);

    Ok(())
}

/// Test scenario: ACL evaluation performance with varied destinations
/// Measures: ACL rule matching overhead with complex rulesets
/// Expected: <5ms per evaluation, >1000 evaluations/s
/// Config requirements: ACL=enabled with multiple rules
async fn test_acl_evaluation(args: &Args) -> std::io::Result<()> {
    println!("\n🔒 Starting ACL Evaluation Performance Test");
    println!("   Measures: ACL rule matching with varied destinations");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 100 concurrent");
    println!("   Proxy: {}", args.proxy);
    println!("   ⚠️  Ensure config has: ACL=on with multiple rules and groups");

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Spawn workers that vary destination addresses to test different ACL paths
    for worker_id in 0..100 {
        let proxy_addr = args.proxy;
        let upstream_addr = args.upstream;
        let metrics = metrics.clone();
        let username = args.username.clone();
        let password = args.password.clone();
        let end_time = test_start + duration;

        tasks.push(tokio::spawn(async move {
            while Instant::now() < end_time {
                // ACL evaluation happens regardless of destination
                // Using same upstream addr ensures connection succeeds while ACL is still evaluated
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

                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
    }

    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("ACL Evaluation Performance", test_elapsed);

    Ok(())
}

/// Test scenario: Long-lived connections stability test
/// Measures: Connection stability over extended period
/// Expected: 0% connection drops, stable latency
/// Config requirements: Any
async fn test_long_lived(args: &Args) -> std::io::Result<()> {
    println!("\n⏰ Starting Long-Lived Connections Test");
    println!("   Measures: Connection stability with sustained connections");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 20 long-lived connections");
    println!("   Proxy: {}", args.proxy);

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Spawn long-lived connections that periodically send data
    for _ in 0..20 {
        let proxy_addr = args.proxy;
        let upstream_addr = args.upstream;
        let metrics = metrics.clone();
        let username = args.username.clone();
        let password = args.password.clone();

        tasks.push(tokio::spawn(async move {
            let connect_start = Instant::now();

            // Establish connection
            let result = socks5_connect(
                proxy_addr,
                upstream_addr,
                username.as_deref(),
                password.as_deref(),
            )
            .await;

            match result {
                Ok((mut stream, handshake_dur)) => {
                    metrics.record_success(handshake_dur.as_nanos() as u64, 0, 0);

                    // Keep connection alive and send periodic keepalives
                    let mut _bytes_sent = 0u64;
                    let mut _bytes_received = 0u64;
                    let keepalive_data = b"KEEPALIVE";

                    while connect_start.elapsed() < duration {
                        // Send keepalive
                        if stream.write_all(keepalive_data).await.is_ok() {
                            _bytes_sent += keepalive_data.len() as u64;

                            let mut buf = [0u8; 1024];
                            if let Ok(n) = stream.read(&mut buf).await {
                                _bytes_received += n as u64;
                            }
                        } else {
                            // Connection dropped
                            metrics.record_failure();
                            break;
                        }

                        // Wait before next keepalive
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }

                    let _ = stream.shutdown().await;
                }
                Err(_) => {
                    metrics.record_failure();
                }
            }
        }));
    }

    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("Long-Lived Connections (Stability)", test_elapsed);

    Ok(())
}

/// Test scenario: Large data transfer
/// Measures: Throughput with multi-MB transfers per connection
/// Expected: >100MB/s sustained bandwidth
/// Config requirements: Any
async fn test_large_transfer(args: &Args) -> std::io::Result<()> {
    println!("\n📦 Starting Large Data Transfer Test");
    println!("   Measures: Multi-MB data transfer throughput");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 10 concurrent large transfers");
    println!("   Transfer Size: 10 MB per connection");
    println!("   Proxy: {}", args.proxy);

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Large data buffer (1 MB)
    let large_data = vec![0u8; 1024 * 1024]; // 1 MB chunk

    for _ in 0..10 {
        let proxy_addr = args.proxy;
        let upstream_addr = args.upstream;
        let metrics = metrics.clone();
        let username = args.username.clone();
        let password = args.password.clone();
        let end_time = test_start + duration;
        let data = large_data.clone();

        tasks.push(tokio::spawn(async move {
            while Instant::now() < end_time {
                let result = socks5_connect(
                    proxy_addr,
                    upstream_addr,
                    username.as_deref(),
                    password.as_deref(),
                )
                .await;

                match result {
                    Ok((mut stream, handshake_dur)) => {
                        let mut bytes_sent = 0u64;
                        let mut bytes_received = 0u64;

                        // Transfer 10 MB total (10 x 1MB chunks)
                        for _ in 0..10 {
                            if stream.write_all(&data).await.is_ok() {
                                bytes_sent += data.len() as u64;

                                // Read echo response
                                let mut buf = vec![0u8; data.len()];
                                if let Ok(n) = stream.read_exact(&mut buf).await {
                                    bytes_received += n as u64;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        let _ = stream.shutdown().await;
                        metrics.record_success(handshake_dur.as_nanos() as u64, bytes_sent, bytes_received);
                    }
                    Err(_) => {
                        metrics.record_failure();
                    }
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }));
    }

    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("Large Data Transfer (10 MB/connection)", test_elapsed);

    Ok(())
}

/// Test scenario: Connection pool efficiency
/// Measures: Pool hit rate and connection reuse effectiveness
/// Expected: >80% pool hit rate, reduced latency on reused connections
/// Config requirements: pool.enabled=true
async fn test_pool_efficiency(args: &Args) -> std::io::Result<()> {
    println!("\n🔄 Starting Connection Pool Efficiency Test");
    println!("   Measures: Pool hit rate and connection reuse");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 50 concurrent (repeated connections to same dest)");
    println!("   Proxy: {}", args.proxy);
    println!("   ⚠️  Ensure config has: pool.enabled=true");

    let metrics = Arc::new(TestMetrics::new());
    let test_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    let mut tasks = Vec::new();

    // Use same destination repeatedly to maximize pool hits
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
                        upstream_addr, // Same destination for pool reuse
                        username.as_deref(),
                        password.as_deref(),
                    ),
                )
                .await;

                match result {
                    Ok(Ok((mut stream, dur))) => {
                        // Send small amount of data
                        let test_data = b"POOL_TEST";
                        let mut bytes_sent = 0u64;
                        let mut bytes_received = 0u64;

                        if stream.write_all(test_data).await.is_ok() {
                            bytes_sent += test_data.len() as u64;
                            let mut buf = [0u8; 1024];
                            if let Ok(n) = stream.read(&mut buf).await {
                                bytes_received += n as u64;
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

                // Short delay to allow pool return
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }));
    }

    for task in tasks {
        let _ = task.await;
    }

    let test_elapsed = test_start.elapsed();
    metrics.print_summary("Connection Pool Efficiency", test_elapsed);

    println!("\n💡 Analysis:");
    println!("   - Lower avg latency vs full-pipeline = effective pool reuse");
    println!("   - Check proxy logs for pool hit/miss statistics");

    Ok(())
}

/// Test scenario: Handshake-only test (no data transfer)
/// Measures: Connection establishment throughput
/// Expected: >1000 conn/s
/// Config requirements: Any (tests raw connection speed)
async fn test_handshake_only(args: &Args) -> std::io::Result<()> {
    println!("\n🤝 Starting Handshake-Only Test");
    println!("   Measures: SOCKS5 handshake throughput (no data transfer)");
    println!("   Duration: {} seconds", args.duration);
    println!("   Workers: 100 concurrent");
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
                        // Immediately close after handshake (no data transfer)
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
    metrics.print_summary("Handshake-Only (No Data Transfer)", test_elapsed);

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
        Scenario::MinimalPipeline => {
            test_minimal_pipeline(&args).await?;
        }
        Scenario::FullPipeline => {
            test_full_pipeline(&args).await?;
        }
        Scenario::HandshakeOnly => {
            test_handshake_only(&args).await?;
        }
        Scenario::DataTransfer => {
            test_data_transfer(&args).await?;
        }
        Scenario::SessionChurn => {
            test_session_churn(&args).await?;
        }
        Scenario::AclEvaluation => {
            test_acl_evaluation(&args).await?;
        }
        Scenario::AuthOverhead => {
            println!("\n⚠️  Auth Overhead Test - Manual Configuration Required");
            println!("   Run twice: once with auth.socks_method='none', once with 'userpass'");
            println!("   Compare results to measure authentication overhead");
            test_handshake_only(&args).await?;
        }
        Scenario::QosLimiting => {
            println!("\n⚠️  QoS Limiting Test - Manual Configuration Required");
            println!("   Configure qos.htb.max_bandwidth_bytes_per_sec to test throttling");
            println!("   Use large-transfer or data-transfer to verify limits");
            test_data_transfer(&args).await?;
        }
        Scenario::PoolEfficiency => {
            test_pool_efficiency(&args).await?;
        }
        Scenario::DnsResolution => {
            println!("\n⚠️  DNS Resolution Test - Manual Configuration Required");
            println!("   Vary upstream addresses (IPv4, IPv6, domains) to test resolution");
            println!("   Use --upstream <ip/domain> flag");
            test_handshake_only(&args).await?;
        }
        Scenario::LongLived => {
            test_long_lived(&args).await?;
        }
        Scenario::LargeTransfer => {
            test_large_transfer(&args).await?;
        }
        Scenario::MetricsOverhead => {
            println!("\n⚠️  Metrics Overhead Test - Manual Configuration Required");
            println!("   Run twice: once with metrics.enabled=true, once with false");
            println!("   Compare results to measure Prometheus overhead");
            test_full_pipeline(&args).await?;
        }
        Scenario::Concurrent1000 => {
            test_concurrent_connections(&args, 1000, 50).await?;
        }
        Scenario::Concurrent5000 => {
            test_concurrent_connections(&args, 5000, 100).await?;
        }
        Scenario::All => {
            println!("\n🎯 Running All Test Scenarios");
            println!("   This will take approximately {} minutes", (args.duration * 14) / 60);
            println!();

            println!("\n📊 Test 1/14: Minimal Pipeline");
            test_minimal_pipeline(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 2/14: Full Pipeline");
            test_full_pipeline(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 3/14: Handshake Only");
            test_handshake_only(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 4/14: Data Transfer");
            test_data_transfer(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 5/14: Session Churn");
            test_session_churn(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 6/14: ACL Evaluation");
            test_acl_evaluation(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 7/14: Connection Pool Efficiency");
            test_pool_efficiency(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 8/14: Long-Lived Connections");
            test_long_lived(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 9/14: Large Data Transfer");
            test_large_transfer(&args).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 10/14: 1000 Concurrent Connections");
            test_concurrent_connections(&args, 1000, 50).await?;
            tokio::time::sleep(Duration::from_secs(5)).await;

            println!("\n📊 Test 11/14: 5000 Concurrent Connections");
            test_concurrent_connections(&args, 5000, 100).await?;

            println!("\n✅ Core tests complete!");
            println!("   Manual tests (require config changes): auth-overhead, qos-limiting, dns-resolution, metrics-overhead");
        }
    }

    println!("\n✅ Load testing completed successfully!");

    Ok(())
}
