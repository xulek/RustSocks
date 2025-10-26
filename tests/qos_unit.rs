//! Comprehensive unit tests for QoS (Quality of Service) subsystem
//!
//! This test suite covers:
//! - TokenBucket: Lock-free token bucket implementation
//! - HtbQos: Hierarchical Token Bucket with fair sharing
//! - QosEngine: High-level QoS engine and configuration

use rustsocks::qos::{ConnectionLimits, HtbConfig, QosConfig, QosEngine};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, Instant};

// ============================================================================
// TokenBucket Tests (via HtbQos - TokenBucket is private)
// ============================================================================

mod token_bucket_tests {
    use super::*;

    #[tokio::test]
    async fn bucket_starts_full_allows_immediate_burst() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 1_000_000,
            max_bandwidth_bytes_per_sec: 5_000_000,
            burst_size_bytes: 500_000, // 500 KB burst
            refill_interval_ms: 50,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .expect("increment connection");

        // Should consume burst immediately without waiting
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 500_000)
            .await
            .expect("allocate burst");
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(10),
            "burst should be instant, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn exceeding_burst_requires_waiting() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000, // 100 KB/s guaranteed
            max_bandwidth_bytes_per_sec: 200_000,        // 200 KB/s max
            burst_size_bytes: 50_000,                    // 50 KB burst
            refill_interval_ms: 10,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .expect("increment connection");

        // First 50KB should be instant (burst)
        qos.allocate_bandwidth("user1", 50_000).await.unwrap();

        // Next 50KB should be instant (second bucket)
        qos.allocate_bandwidth("user1", 50_000).await.unwrap();

        // Third 50KB should require waiting for refill
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 50_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(100),
            "expected throttling delay, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn refill_mechanism_restores_tokens_over_time() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000, // 100 KB/s = 100 KB per second
            max_bandwidth_bytes_per_sec: 200_000,
            burst_size_bytes: 100_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .expect("increment connection");

        // Consume all tokens
        qos.allocate_bandwidth("user1", 100_000).await.unwrap();
        qos.allocate_bandwidth("user1", 100_000).await.unwrap();

        // Wait for refill (100ms should refill ~10KB at 100KB/s)
        sleep(Duration::from_millis(100)).await;

        // Should be able to consume refilled tokens quickly
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 10_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(50),
            "refilled tokens should be available, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn concurrent_consumption_from_multiple_tasks() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 500_000,
            max_bandwidth_bytes_per_sec: 1_000_000,
            burst_size_bytes: 1_000_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = Arc::new(
            QosEngine::from_config(QosConfig {
                enabled: true,
                algorithm: "htb".to_string(),
                htb: config.clone(),
                connection_limits: ConnectionLimits::default(),
            })
            .await
            .expect("create QoS engine"),
        );

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .expect("increment connection");

        // Spawn 10 concurrent tasks consuming bandwidth
        let mut handles = vec![];
        for _ in 0..10 {
            let qos_clone = qos.clone();
            let handle = tokio::spawn(async move {
                qos_clone
                    .allocate_bandwidth("user1", 50_000)
                    .await
                    .expect("allocate bandwidth");
            });
            handles.push(handle);
        }

        // All should complete without panics
        for handle in handles {
            handle.await.expect("task completed");
        }
    }

    #[tokio::test]
    async fn zero_consumption_succeeds_immediately() {
        let config = HtbConfig::default();

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config,
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .expect("increment connection");

        // Zero consumption should always succeed
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 0).await.unwrap();
        let elapsed = start.elapsed();

        assert!(elapsed < Duration::from_millis(1));
    }
}

// ============================================================================
// HtbQos Connection Counting Tests
// ============================================================================

mod connection_counting_tests {
    use super::*;

    #[tokio::test]
    async fn connection_count_starts_at_zero() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        assert_eq!(qos.get_user_connections("alice"), 0);
        assert_eq!(qos.get_total_connections(), 0);
    }

    #[tokio::test]
    async fn increment_connections_updates_counts() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .expect("increment alice");
        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .expect("increment alice");
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .expect("increment bob");

        assert_eq!(qos.get_user_connections("alice"), 2);
        assert_eq!(qos.get_user_connections("bob"), 1);
        assert_eq!(qos.get_total_connections(), 3);
    }

    #[tokio::test]
    async fn decrement_connections_updates_counts() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        qos.dec_user_connection("alice");
        assert_eq!(qos.get_user_connections("alice"), 1);
        assert_eq!(qos.get_total_connections(), 2);

        qos.dec_user_connection("bob");
        assert_eq!(qos.get_user_connections("bob"), 0);
        assert_eq!(qos.get_total_connections(), 1);
    }

    #[tokio::test]
    async fn decrement_nonexistent_user_returns_zero() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.dec_user_connection("ghost");
        assert_eq!(qos.get_user_connections("ghost"), 0);
    }

    #[tokio::test]
    async fn per_user_limit_enforced() {
        let limits = ConnectionLimits {
            max_connections_per_user: 2,
            max_connections_global: 100,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: limits.clone(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &limits).unwrap();
        qos.check_and_inc_connection("alice", &limits).unwrap();

        let result = qos.check_and_inc_connection("alice", &limits);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("User connection limit"));
    }

    #[tokio::test]
    async fn global_limit_enforced() {
        let limits = ConnectionLimits {
            max_connections_per_user: 100,
            max_connections_global: 3,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: limits.clone(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &limits).unwrap();
        qos.check_and_inc_connection("bob", &limits).unwrap();
        qos.check_and_inc_connection("charlie", &limits).unwrap();

        let result = qos.check_and_inc_connection("dave", &limits);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Global connection limit"));
    }

    #[tokio::test]
    async fn concurrent_increment_operations_safe() {
        let qos = Arc::new(
            QosEngine::from_config(QosConfig {
                enabled: true,
                algorithm: "htb".to_string(),
                htb: HtbConfig::default(),
                connection_limits: ConnectionLimits::default(),
            })
            .await
            .expect("create QoS engine"),
        );

        // Spawn 100 concurrent increments
        let mut handles = vec![];
        for i in 0..100 {
            let qos_clone = qos.clone();
            let user = format!("user{}", i % 10); // 10 different users
            let handle = tokio::spawn(async move {
                qos_clone
                    .check_and_inc_connection(&user, &ConnectionLimits::default())
                    .expect("increment connection");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.expect("task completed");
        }

        // Should have exactly 100 total connections
        assert_eq!(qos.get_total_connections(), 100);
    }
}

// ============================================================================
// HtbQos Bandwidth Allocation Tests
// ============================================================================

mod bandwidth_allocation_tests {
    use super::*;

    #[tokio::test]
    async fn guaranteed_bandwidth_always_available() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 1_000_000, // 1 MB/s guaranteed
            max_bandwidth_bytes_per_sec: 5_000_000,
            burst_size_bytes: 1_000_000,
            refill_interval_ms: 50,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .unwrap();

        // Consume guaranteed bucket (burst)
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 1_000_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(10),
            "guaranteed burst should be instant, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn can_borrow_beyond_guaranteed() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000,
            max_bandwidth_bytes_per_sec: 500_000, // Can borrow up to 500 KB/s
            burst_size_bytes: 100_000,
            refill_interval_ms: 50,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .unwrap();

        // Consume guaranteed bucket
        qos.allocate_bandwidth("user1", 100_000).await.unwrap();

        // Should be able to borrow from max bucket
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 100_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(20),
            "borrowing should use max bucket burst, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn global_bandwidth_limit_enforced() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 100_000, // Very low global limit
            guaranteed_bandwidth_bytes_per_sec: 50_000,
            max_bandwidth_bytes_per_sec: 500_000,
            burst_size_bytes: 100_000,
            refill_interval_ms: 50,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = Arc::new(
            QosEngine::from_config(QosConfig {
                enabled: true,
                algorithm: "htb".to_string(),
                htb: config.clone(),
                connection_limits: ConnectionLimits::default(),
            })
            .await
            .expect("create QoS engine"),
        );

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("user2", &ConnectionLimits::default())
            .unwrap();

        // Consume global burst
        qos.allocate_bandwidth("user1", 100_000).await.unwrap();

        // Next allocation should wait for global bucket refill
        let start = Instant::now();
        qos.allocate_bandwidth("user2", 50_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(100),
            "global limit should throttle, took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn multiple_users_consume_independently() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000,
            max_bandwidth_bytes_per_sec: 500_000,
            burst_size_bytes: 100_000,
            refill_interval_ms: 50,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = Arc::new(
            QosEngine::from_config(QosConfig {
                enabled: true,
                algorithm: "htb".to_string(),
                htb: config.clone(),
                connection_limits: ConnectionLimits::default(),
            })
            .await
            .expect("create QoS engine"),
        );

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        // Both should be able to consume their burst independently
        let qos_clone = qos.clone();
        let alice_task = tokio::spawn(async move {
            let start = Instant::now();
            qos_clone.allocate_bandwidth("alice", 100_000).await.unwrap();
            start.elapsed()
        });

        let qos_clone = qos.clone();
        let bob_task = tokio::spawn(async move {
            let start = Instant::now();
            qos_clone.allocate_bandwidth("bob", 100_000).await.unwrap();
            start.elapsed()
        });

        let alice_time = alice_task.await.unwrap();
        let bob_time = bob_task.await.unwrap();

        assert!(alice_time < Duration::from_millis(20));
        assert!(bob_time < Duration::from_millis(20));
    }
}

// ============================================================================
// Fair Sharing Tests
// ============================================================================

mod fair_sharing_tests {
    use super::*;

    #[tokio::test]
    async fn fair_sharing_balances_bandwidth() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000, // 1 MB/s total
            guaranteed_bandwidth_bytes_per_sec: 200_000, // 200 KB/s guaranteed
            max_bandwidth_bytes_per_sec: 800_000,       // 800 KB/s max
            burst_size_bytes: 200_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: true,
            rebalance_interval_ms: 50, // Rebalance quickly
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        // Activate two users
        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        // Both consume to show demand
        qos.allocate_bandwidth("alice", 200_000).await.unwrap();
        qos.allocate_bandwidth("bob", 200_000).await.unwrap();

        // Wait for rebalancing
        sleep(Duration::from_millis(150)).await;

        let allocations = qos.get_user_allocations().await;
        let alice_alloc = allocations.iter().find(|a| a.user == "alice");
        let bob_alloc = allocations.iter().find(|a| a.user == "bob");

        assert!(alice_alloc.is_some());
        assert!(bob_alloc.is_some());

        // Both should have similar allocations (within 70%)
        let alice_bw = alice_alloc.unwrap().allocated_bandwidth;
        let bob_bw = bob_alloc.unwrap().allocated_bandwidth;

        let diff_ratio = alice_bw.abs_diff(bob_bw) as f64 / alice_bw.max(bob_bw) as f64;

        assert!(
            diff_ratio < 0.7,
            "expected fair allocation, got alice={} bob={} (diff={:.2}%)",
            alice_bw,
            bob_bw,
            diff_ratio * 100.0
        );
    }

    #[tokio::test]
    async fn high_demand_user_gets_more_bandwidth() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000,
            max_bandwidth_bytes_per_sec: 800_000,
            burst_size_bytes: 100_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: true,
            rebalance_interval_ms: 50,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        // Alice shows high demand by consuming multiple bursts
        qos.allocate_bandwidth("alice", 100_000).await.unwrap();
        qos.allocate_bandwidth("alice", 100_000).await.unwrap();

        // Bob shows low demand
        qos.allocate_bandwidth("bob", 10_000).await.unwrap();

        // Wait for rebalancing to detect demand
        sleep(Duration::from_millis(150)).await;

        let allocations = qos.get_user_allocations().await;
        let alice_alloc = allocations
            .iter()
            .find(|a| a.user == "alice")
            .expect("alice allocation");
        let bob_alloc = allocations
            .iter()
            .find(|a| a.user == "bob")
            .expect("bob allocation");

        // Alice should get more bandwidth due to higher demand
        assert!(
            alice_alloc.allocated_bandwidth >= bob_alloc.allocated_bandwidth,
            "high-demand user should get more, alice={} bob={}",
            alice_alloc.allocated_bandwidth,
            bob_alloc.allocated_bandwidth
        );
    }

    #[tokio::test]
    async fn idle_user_not_allocated_bandwidth() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,
            guaranteed_bandwidth_bytes_per_sec: 200_000,
            max_bandwidth_bytes_per_sec: 800_000,
            burst_size_bytes: 200_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: true,
            rebalance_interval_ms: 50,
            idle_timeout_secs: 1, // Short timeout for test
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        // Alice is active
        qos.allocate_bandwidth("alice", 50_000).await.unwrap();

        // Bob goes idle (wait past timeout)
        sleep(Duration::from_millis(1500)).await;

        // Keep Alice active with another allocation
        qos.allocate_bandwidth("alice", 10_000).await.unwrap();

        let allocations = qos.get_user_allocations().await;
        let alice_alloc = allocations.iter().find(|a| a.user == "alice");
        let bob_alloc = allocations.iter().find(|a| a.user == "bob");

        assert!(alice_alloc.unwrap().is_active);
        assert!(!bob_alloc.unwrap().is_active);
    }

    #[tokio::test]
    async fn disabled_fair_sharing_uses_static_limits() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,
            guaranteed_bandwidth_bytes_per_sec: 200_000,
            max_bandwidth_bytes_per_sec: 500_000,
            burst_size_bytes: 200_000,
            refill_interval_ms: 10,
            fair_sharing_enabled: false, // Disabled
            rebalance_interval_ms: 50,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        // Show different demand levels
        qos.allocate_bandwidth("alice", 200_000).await.unwrap();
        qos.allocate_bandwidth("bob", 10_000).await.unwrap();

        // Wait for potential rebalancing (shouldn't happen)
        sleep(Duration::from_millis(150)).await;

        let allocations = qos.get_user_allocations().await;

        // All active users should have same max_bandwidth (static)
        for alloc in allocations {
            if alloc.active_connections > 0 {
                assert_eq!(alloc.max_bandwidth, 500_000);
            }
        }
    }
}

// ============================================================================
// QosEngine Configuration Tests
// ============================================================================

mod qos_engine_tests {
    use super::*;

    #[tokio::test]
    async fn disabled_qos_allows_unlimited_traffic() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: false,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        assert!(!qos.is_enabled());

        // Should allocate instantly without any limits
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 999_999_999).await.unwrap();
        let elapsed = start.elapsed();

        assert!(elapsed < Duration::from_millis(1));
    }

    #[tokio::test]
    async fn unknown_algorithm_returns_error() {
        let result = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "unknown-algo".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Unknown QoS algorithm"));
        }
    }

    #[tokio::test]
    async fn htb_algorithm_creates_htb_engine() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        assert!(qos.is_enabled());
    }

    #[tokio::test]
    async fn default_config_values_applied() {
        let config = QosConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.algorithm, "htb");
        assert_eq!(config.htb.global_bandwidth_bytes_per_sec, 125_000_000); // 1 Gbps
        assert_eq!(config.htb.guaranteed_bandwidth_bytes_per_sec, 131_072); // 1 Mbps
        assert_eq!(config.htb.max_bandwidth_bytes_per_sec, 12_500_000); // 100 Mbps
        assert_eq!(config.htb.burst_size_bytes, 1_048_576); // 1 MB
        assert_eq!(config.htb.refill_interval_ms, 50);
        assert!(config.htb.fair_sharing_enabled);
        assert_eq!(config.htb.rebalance_interval_ms, 100);
        assert_eq!(config.htb.idle_timeout_secs, 5);
        assert_eq!(config.connection_limits.max_connections_per_user, 20);
        assert_eq!(config.connection_limits.max_connections_global, 10_000);
    }

    #[tokio::test]
    async fn custom_config_values_respected() {
        let custom_htb = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,
            guaranteed_bandwidth_bytes_per_sec: 100_000,
            max_bandwidth_bytes_per_sec: 500_000,
            burst_size_bytes: 50_000,
            refill_interval_ms: 25,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 200,
            idle_timeout_secs: 10,
        };

        let custom_limits = ConnectionLimits {
            max_connections_per_user: 5,
            max_connections_global: 50,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: custom_htb.clone(),
            connection_limits: custom_limits.clone(),
        })
        .await
        .expect("create QoS engine");

        // Verify limits are enforced
        for i in 0..5 {
            qos.check_and_inc_connection("user1", &custom_limits)
                .expect(&format!("connection {}", i));
        }

        let result = qos.check_and_inc_connection("user1", &custom_limits);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("User connection limit"));
    }
}

// ============================================================================
// User Allocation Information Tests
// ============================================================================

mod user_allocation_tests {
    use super::*;

    #[tokio::test]
    async fn get_user_allocations_returns_active_users() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.check_and_inc_connection("bob", &ConnectionLimits::default())
            .unwrap();

        qos.allocate_bandwidth("alice", 1000).await.unwrap();
        qos.allocate_bandwidth("bob", 1000).await.unwrap();

        let allocations = qos.get_user_allocations().await;

        assert_eq!(allocations.len(), 2);

        let alice = allocations.iter().find(|a| a.user == "alice").unwrap();
        let bob = allocations.iter().find(|a| a.user == "bob").unwrap();

        assert_eq!(alice.active_connections, 1);
        assert_eq!(bob.active_connections, 1);
        assert!(alice.guaranteed_bandwidth > 0);
        assert!(bob.guaranteed_bandwidth > 0);
    }

    #[tokio::test]
    async fn allocation_includes_all_metadata() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1_000_000,
            guaranteed_bandwidth_bytes_per_sec: 200_000,
            max_bandwidth_bytes_per_sec: 800_000,
            burst_size_bytes: 100_000,
            refill_interval_ms: 50,
            fair_sharing_enabled: true,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config.clone(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.allocate_bandwidth("alice", 1000).await.unwrap();

        let allocations = qos.get_user_allocations().await;
        let alice = allocations.iter().find(|a| a.user == "alice").unwrap();

        assert_eq!(alice.user, "alice");
        assert_eq!(alice.guaranteed_bandwidth, 200_000);
        assert_eq!(alice.max_bandwidth, 800_000);
        assert!(alice.is_active);
        assert_eq!(alice.active_connections, 1);
        assert!(alice.allocated_bandwidth > 0);
    }

    #[tokio::test]
    async fn empty_allocations_when_no_users() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        let allocations = qos.get_user_allocations().await;
        assert_eq!(allocations.len(), 0);
    }

    #[tokio::test]
    async fn allocations_persist_after_disconnect() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("alice", &ConnectionLimits::default())
            .unwrap();
        qos.allocate_bandwidth("alice", 1000).await.unwrap();

        qos.dec_user_connection("alice");
        assert_eq!(qos.get_user_connections("alice"), 0);

        // Allocation record still exists (though inactive)
        let allocations = qos.get_user_allocations().await;
        let alice = allocations.iter().find(|a| a.user == "alice");
        assert!(alice.is_some());
    }
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

mod edge_cases_tests {
    use super::*;

    #[tokio::test]
    async fn very_small_bandwidth_limits_work() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 1000, // 1 KB/s
            guaranteed_bandwidth_bytes_per_sec: 100,
            max_bandwidth_bytes_per_sec: 500,
            burst_size_bytes: 100,
            refill_interval_ms: 10,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config,
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .unwrap();

        // Should still work, just with throttling
        qos.allocate_bandwidth("user1", 100).await.unwrap();
    }

    #[tokio::test]
    async fn very_large_bandwidth_limits_work() {
        let config = HtbConfig {
            global_bandwidth_bytes_per_sec: 10_000_000_000, // 10 GB/s
            guaranteed_bandwidth_bytes_per_sec: 1_000_000_000,
            max_bandwidth_bytes_per_sec: 5_000_000_000,
            burst_size_bytes: 1_000_000_000,
            refill_interval_ms: 50,
            fair_sharing_enabled: false,
            rebalance_interval_ms: 100,
            idle_timeout_secs: 5,
        };

        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: config,
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        qos.check_and_inc_connection("user1", &ConnectionLimits::default())
            .unwrap();

        // Should allocate large amounts instantly
        let start = Instant::now();
        qos.allocate_bandwidth("user1", 1_000_000_000).await.unwrap();
        let elapsed = start.elapsed();

        assert!(elapsed < Duration::from_millis(10));
    }

    #[tokio::test]
    async fn rapid_connect_disconnect_cycles() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        // Rapid cycles
        for _ in 0..100 {
            qos.check_and_inc_connection("user1", &ConnectionLimits::default())
                .unwrap();
            qos.dec_user_connection("user1");
        }

        assert_eq!(qos.get_user_connections("user1"), 0);
        assert_eq!(qos.get_total_connections(), 0);
    }

    #[tokio::test]
    async fn username_with_special_characters() {
        let qos = QosEngine::from_config(QosConfig {
            enabled: true,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        })
        .await
        .expect("create QoS engine");

        let special_users = vec![
            "user@example.com",
            "user-with-dashes",
            "user_with_underscores",
            "user.with.dots",
            "user123",
            "UPPERCASE",
            "MixedCase",
            "user with spaces",
            "用户", // Unicode
        ];

        for user in special_users {
            qos.check_and_inc_connection(user, &ConnectionLimits::default())
                .expect(&format!("failed for user: {}", user));
            qos.allocate_bandwidth(user, 1000).await.expect(&format!("failed allocation for: {}", user));
            assert_eq!(qos.get_user_connections(user), 1);
            qos.dec_user_connection(user);
        }
    }

    #[tokio::test]
    async fn stress_test_many_concurrent_users() {
        let qos = Arc::new(
            QosEngine::from_config(QosConfig {
                enabled: true,
                algorithm: "htb".to_string(),
                htb: HtbConfig::default(),
                connection_limits: ConnectionLimits {
                    max_connections_per_user: 100,
                    max_connections_global: 10000,
                },
            })
            .await
            .expect("create QoS engine"),
        );

        let mut handles = vec![];

        // Create 50 concurrent users, each making 10 allocations
        for user_id in 0..50 {
            let qos_clone = qos.clone();
            let handle = tokio::spawn(async move {
                let user = format!("user{}", user_id);
                let limits = ConnectionLimits {
                    max_connections_per_user: 100,
                    max_connections_global: 10000,
                };

                qos_clone
                    .check_and_inc_connection(&user, &limits)
                    .expect("increment connection");

                for _ in 0..10 {
                    qos_clone
                        .allocate_bandwidth(&user, 10_000)
                        .await
                        .expect("allocate bandwidth");
                }

                qos_clone.dec_user_connection(&user);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.expect("task completed");
        }

        assert_eq!(qos.get_total_connections(), 0);
    }
}
