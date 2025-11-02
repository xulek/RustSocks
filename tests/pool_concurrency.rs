/// Connection Pool Concurrency Stress Tests
///
/// Tests pool performance under high concurrent load

use rustsocks::server::{ConnectionPool, PoolConfig};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

#[tokio::test]
#[ignore] // Stress test - run with --ignored
async fn pool_handles_hundred_concurrent_gets() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 10,
        max_total_idle: 500,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Start echo servers on different ports
    let mut servers = Vec::new();
    for _ in 0..5 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4];
                        if tokio::io::AsyncReadExt::read_exact(&mut stream, &mut buf).await.is_ok() {
                            tokio::io::AsyncWriteExt::write_all(&mut stream, &buf).await.ok();
                        }
                    });
                }
            }
        });

        servers.push(addr);
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Concurrent get operations
    let start = Instant::now();
    let mut tasks = Vec::new();

    for i in 0..200 {
        let pool_clone = pool.clone();
        let addr = servers[i % servers.len()];

        tasks.push(tokio::spawn(async move {
            pool_clone.get(addr).await
        }));
    }

    let mut successes = 0;
    let mut failures = 0;

    for task in tasks {
        match task.await {
            Ok(Ok(_stream)) => successes += 1,
            Ok(Err(_)) => failures += 1,
            Err(_) => failures += 1,
        }
    }

    let elapsed = start.elapsed();

    println!("=== Concurrent Get Test ===");
    println!("Total requests: 200");
    println!("Successes: {}", successes);
    println!("Failures: {}", failures);
    println!("Elapsed: {:?}", elapsed);
    println!("Avg per request: {:?}", elapsed / 200);

    assert!(successes > 190, "Should have >95% success rate");
    assert!(elapsed.as_millis() < 5000, "Should complete within 5 seconds");
}

#[tokio::test]
#[ignore] // Stress test
async fn pool_put_get_concurrent_stress() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 20,
        max_total_idle: 1000,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    // Single echo server
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4];
                    if tokio::io::AsyncReadExt::read_exact(&mut stream, &mut buf).await.is_ok() {
                        tokio::io::AsyncWriteExt::write_all(&mut stream, &buf).await.ok();
                    }
                });
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let start = Instant::now();

    // Simulate 500 concurrent connection cycles (get → use → put)
    let mut tasks = Vec::new();
    for _ in 0..500 {
        let pool_clone = pool.clone();

        tasks.push(tokio::spawn(async move {
            // Get connection
            let stream = pool_clone.get(server_addr).await?;

            // Simulate usage
            tokio::time::sleep(tokio::time::Duration::from_micros(100)).await;

            // Put back to pool
            pool_clone.put(server_addr, stream).await;

            Ok::<_, std::io::Error>(())
        }));
    }

    let mut completed = 0;
    for task in tasks {
        if task.await.is_ok() {
            completed += 1;
        }
    }

    let elapsed = start.elapsed();

    println!("=== Get/Put Cycle Test ===");
    println!("Total cycles: 500");
    println!("Completed: {}", completed);
    println!("Elapsed: {:?}", elapsed);
    println!("Throughput: {:.2} ops/sec", 500.0 / elapsed.as_secs_f64());

    assert!(completed > 480, "Should complete >96% of cycles");

    // Check pool stats
    let stats = pool.stats().await;
    println!("Pool stats: {} idle in {} destinations", stats.total_idle, stats.destinations);
    assert!(stats.total_idle <= stats.config.max_total_idle);
}

#[tokio::test]
#[ignore] // Performance benchmark
async fn pool_mutex_contention_benchmark() {
    let pool_config = PoolConfig {
        enabled: true,
        max_idle_per_dest: 5,
        max_total_idle: 100,
        idle_timeout_secs: 90,
        connect_timeout_ms: 5000,
    };
    let pool = Arc::new(ConnectionPool::new(pool_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Measure lock contention
    let iterations = 1000;
    let concurrency_levels = vec![1, 10, 50, 100, 200];

    for concurrency in concurrency_levels {
        let start = Instant::now();
        let mut tasks = Vec::new();

        for _ in 0..concurrency {
            let pool_clone = pool.clone();

            tasks.push(tokio::spawn(async move {
                for _ in 0..iterations / concurrency {
                    let _ = pool_clone.get(server_addr).await;
                }
            }));
        }

        for task in tasks {
            task.await.ok();
        }

        let elapsed = start.elapsed();
        let ops_per_sec = iterations as f64 / elapsed.as_secs_f64();

        println!(
            "Concurrency: {:3} | Total: {:?} | Throughput: {:.0} ops/sec",
            concurrency, elapsed, ops_per_sec
        );
    }

    // If throughput degrades significantly with concurrency, we have contention
    println!("\n⚠️  If throughput drops >50% from concurrency=1 to concurrency=200,");
    println!("    consider using DashMap instead of Mutex<HashMap>");
}
