// Session Manager Edge Cases Tests
// Tests for concurrent operations, boundary conditions, and stress testing

use rustsocks::session::manager::SessionManager;
use rustsocks::session::types::{ConnectionInfo, SessionStatus};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

fn create_test_connection(source_port: u16, dest_port: u16) -> ConnectionInfo {
    ConnectionInfo {
        source_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
        source_port,
        dest_ip: "93.184.216.34".to_string(), // example.com
        dest_port,
        protocol: rustsocks::session::types::Protocol::Tcp,
    }
}

#[tokio::test]
async fn test_concurrent_session_creation() {
    let manager = Arc::new(SessionManager::new());
    let mut handles = vec![];

    // Create 100 sessions concurrently
    for i in 0..100 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            let conn = create_test_connection(1000 + i, 80);
            mgr.new_session(&format!("user{}", i), conn, "allow", None)
                .await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut session_ids = vec![];
    for handle in handles {
        let session_id = handle.await.unwrap();
        session_ids.push(session_id);
    }

    // Verify all sessions are unique and tracked
    assert_eq!(session_ids.len(), 100);
    let unique_count = session_ids
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique_count, 100);

    // Verify active session count
    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 100);
}

#[tokio::test]
async fn test_concurrent_traffic_updates() {
    let manager = Arc::new(SessionManager::new());
    let conn = create_test_connection(2000, 443);
    let session_id = manager.new_session("alice", conn, "allow", None).await;

    let mut handles = vec![];

    // Update traffic from 50 concurrent tasks
    for i in 0..50 {
        let mgr = manager.clone();
        let sid = session_id;
        let handle = tokio::spawn(async move {
            for _ in 0..20 {
                mgr.update_traffic(&sid, 100 * (i + 1), 50 * (i + 1), 10, 5)
                    .await;
                // Small delay to increase contention
                tokio::time::sleep(Duration::from_micros(1)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all updates
    for handle in handles {
        handle.await.unwrap();
    }

    // Close session and verify total traffic
    manager
        .close_session(&session_id, None, SessionStatus::Closed)
        .await;

    let snapshots = manager.get_closed_sessions().await;
    assert_eq!(snapshots.len(), 1);

    // Traffic should be accumulated correctly despite concurrent updates
    assert!(snapshots[0].bytes_sent > 0);
    assert!(snapshots[0].bytes_received > 0);
    assert!(snapshots[0].packets_sent > 0);
    assert!(snapshots[0].packets_received > 0);
}

#[tokio::test]
async fn test_concurrent_session_close() {
    let manager = Arc::new(SessionManager::new());

    // Create 50 sessions
    let mut session_ids = vec![];
    for i in 0..50 {
        let conn = create_test_connection(3000 + i, 80);
        let session_id = manager
            .new_session(&format!("user{}", i % 10), conn, "allow", None)
            .await;
        session_ids.push(session_id);
    }

    // Close all sessions concurrently
    let mut handles = vec![];
    for session_id in session_ids {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            mgr.close_session(
                &session_id,
                Some("concurrent_close".to_string()),
                SessionStatus::Closed,
            )
            .await;
        });
        handles.push(handle);
    }

    // Wait for all closes
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all sessions are closed
    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 0);

    let closed = manager.get_closed_sessions().await;
    assert_eq!(closed.len(), 50);

    // All should have close reason
    for session in closed {
        assert_eq!(session.close_reason, Some("concurrent_close".to_string()));
    }
}

#[tokio::test]
async fn test_session_with_maximum_traffic() {
    let manager = SessionManager::new();
    let conn = create_test_connection(4000, 80);
    let session_id = manager.new_session("alice", conn, "allow", None).await;

    // Update with very large traffic values
    let max_bytes = u64::MAX / 2; // Use half of max to avoid overflow in accumulation
    manager
        .update_traffic(&session_id, max_bytes, max_bytes, 1_000_000, 1_000_000)
        .await;

    manager
        .close_session(&session_id, None, SessionStatus::Closed)
        .await;

    let closed = manager.get_closed_sessions().await;
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].bytes_sent, max_bytes);
    assert_eq!(closed[0].bytes_received, max_bytes);
}

#[tokio::test]
async fn test_session_with_zero_traffic() {
    let manager = SessionManager::new();
    let conn = create_test_connection(5000, 80);
    let session_id = manager.new_session("bob", conn, "allow", None).await;

    // Close immediately without any traffic
    manager
        .close_session(
            &session_id,
            Some("no_traffic".to_string()),
            SessionStatus::Closed,
        )
        .await;

    let closed = manager.get_closed_sessions().await;
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].bytes_sent, 0);
    assert_eq!(closed[0].bytes_received, 0);
    assert_eq!(closed[0].packets_sent, 0);
    assert_eq!(closed[0].packets_received, 0);
}

#[tokio::test]
async fn test_rejected_sessions_tracking() {
    let manager = SessionManager::new();

    // Track 25 rejected sessions
    for i in 0..25 {
        let conn = create_test_connection(6000 + i, 443);
        manager
            .track_rejected_session(
                &format!("user{}", i % 5),
                conn,
                Some(format!("rule_{}", i % 3)),
            )
            .await;
    }

    let rejected = manager.rejected_snapshot().await;
    assert_eq!(rejected.len(), 25);

    // All should have rejected status
    for session in rejected {
        assert_eq!(session.status, SessionStatus::RejectedByAcl);
        assert!(session.acl_rule_matched.is_some());
    }
}

#[tokio::test]
async fn test_concurrent_rejected_and_accepted_sessions() {
    let manager = Arc::new(SessionManager::new());
    let mut handles = vec![];

    // Mix of accepted and rejected sessions
    for i in 0..100 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            let conn = create_test_connection(7000 + i, 80);
            if i % 2 == 0 {
                // Accept
                mgr.new_session(&format!("user{}", i), conn, "allow", None)
                    .await
            } else {
                // Reject
                mgr.track_rejected_session(
                    &format!("user{}", i),
                    conn,
                    Some("test_rule".to_string()),
                )
                .await
            }
        });
        handles.push(handle);
    }

    // Wait for all
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify counts
    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 50); // Half accepted

    let rejected = manager.rejected_snapshot().await;
    assert_eq!(rejected.len(), 50); // Half rejected
}

#[tokio::test]
async fn test_session_stats_with_multiple_users() {
    let manager = SessionManager::new();

    // Create sessions for multiple users with different traffic
    let users = ["alice", "bob", "charlie", "diana"];
    let mut session_ids = vec![];

    for (i, user) in users.iter().enumerate() {
        for j in 0..5 {
            let conn = create_test_connection(8000 + (i * 10 + j) as u16, 443);
            let session_id = manager.new_session(user, conn, "allow", None).await;

            // Different traffic for each user
            let bytes = (i as u64 + 1) * 1000 * (j + 1) as u64;
            manager
                .update_traffic(&session_id, bytes, bytes / 2, 10, 5)
                .await;

            session_ids.push(session_id);
        }
    }

    // Close all sessions
    for session_id in session_ids {
        manager
            .close_session(&session_id, None, SessionStatus::Closed)
            .await;
    }

    // Get stats
    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 0);
    assert_eq!(stats.total_sessions, 20);

    // Verify top users
    assert_eq!(stats.top_users.len(), 4);
}

#[tokio::test]
async fn test_session_with_very_long_close_reason() {
    let manager = SessionManager::new();
    let conn = create_test_connection(9000, 80);
    let session_id = manager.new_session("alice", conn, "allow", None).await;

    // Close with very long reason (1000 chars)
    let long_reason = "x".repeat(1000);
    manager
        .close_session(
            &session_id,
            Some(long_reason.clone()),
            SessionStatus::Closed,
        )
        .await;

    let closed = manager.get_closed_sessions().await;
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].close_reason, Some(long_reason));
}

#[tokio::test]
async fn test_session_duration_calculation() {
    let manager = SessionManager::new();
    let conn = create_test_connection(10000, 80);
    let session_id = manager.new_session("alice", conn, "allow", None).await;

    // Wait a bit
    sleep(Duration::from_millis(100)).await;

    manager
        .close_session(&session_id, None, SessionStatus::Closed)
        .await;

    let closed = manager.get_closed_sessions().await;
    assert_eq!(closed.len(), 1);

    // Duration should be present (we slept for 100ms)
    assert!(
        closed[0].duration_secs.is_some(),
        "Duration was {:?}",
        closed[0].duration_secs
    );
}

#[tokio::test]
async fn test_get_session_by_id() {
    let manager = SessionManager::new();
    let conn = create_test_connection(11000, 443);
    let session_id = manager.new_session("bob", conn, "allow", None).await;

    // Update traffic
    manager
        .update_traffic(&session_id, 5000, 3000, 50, 30)
        .await;

    // Get session
    let session_arc = manager.get_session(&session_id);
    assert!(session_arc.is_some());

    let arc = session_arc.unwrap();
    let session = arc.read().await;
    assert_eq!(session.user.as_ref(), "bob");
    assert_eq!(session.bytes_sent, 5000);
    assert_eq!(session.bytes_received, 3000);
    assert_eq!(session.status, SessionStatus::Active);
}

#[tokio::test]
async fn test_get_nonexistent_session() {
    let manager = SessionManager::new();
    let fake_id = uuid::Uuid::new_v4();

    let session = manager.get_session(&fake_id);
    assert!(session.is_none());
}

#[tokio::test]
async fn test_stats_time_window_filtering() {
    let manager = SessionManager::new();

    // Create and close sessions
    for i in 0..10 {
        let conn = create_test_connection(12000 + i, 80);
        let session_id = manager.new_session("alice", conn, "allow", None).await;
        manager.update_traffic(&session_id, 1000, 500, 10, 5).await;
        manager
            .close_session(&session_id, None, SessionStatus::Closed)
            .await;
    }

    // Get stats with different windows
    let stats_24h = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    let stats_1h = manager.get_stats(Duration::from_secs(3600)).await;

    // Both should see the same sessions (just created)
    assert_eq!(stats_24h.total_sessions, 10);
    assert_eq!(stats_1h.total_sessions, 10);
    assert_eq!(stats_24h.total_bytes, 15000); // 10 sessions * (1000 sent + 500 received)
    assert_eq!(stats_1h.total_bytes, 15000);
}

#[tokio::test]
async fn test_concurrent_get_stats() {
    let manager = Arc::new(SessionManager::new());

    // Create some sessions
    for i in 0..20 {
        let conn = create_test_connection(13000 + i, 443);
        manager.new_session("alice", conn, "allow", None).await;
    }

    // Get stats concurrently from multiple tasks
    let mut handles = vec![];
    for _ in 0..50 {
        let mgr = manager.clone();
        let handle =
            tokio::spawn(async move { mgr.get_stats(Duration::from_secs(24 * 3600)).await });
        handles.push(handle);
    }

    // All should succeed
    for handle in handles {
        let stats = handle.await.unwrap();
        assert_eq!(stats.active_sessions, 20);
    }
}

#[tokio::test]
async fn test_session_active_count() {
    let manager = SessionManager::new();

    // Create many sessions
    for i in 0..100 {
        let conn = create_test_connection(14000 + i, 80);
        manager
            .new_session(&format!("user{}", i % 10), conn, "allow", None)
            .await;
    }

    // Check active count
    assert_eq!(manager.active_session_count(), 100);

    // Get stats to verify
    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 100);
}

#[tokio::test]
async fn test_mixed_protocol_sessions() {
    let manager = SessionManager::new();

    // Create TCP sessions
    for i in 0..10 {
        let conn = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            source_port: 15000 + i,
            dest_ip: "93.184.216.34".to_string(),
            dest_port: 80,
            protocol: rustsocks::session::types::Protocol::Tcp,
        };
        manager.new_session("alice", conn, "allow", None).await;
    }

    // Create UDP sessions
    for i in 0..10 {
        let conn = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            source_port: 16000 + i,
            dest_ip: "93.184.216.34".to_string(),
            dest_port: 53,
            protocol: rustsocks::session::types::Protocol::Udp,
        };
        manager.new_session("bob", conn, "allow", None).await;
    }

    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 20);
}

#[tokio::test]
#[ignore] // Stress test - run with --ignored flag
async fn test_stress_many_sessions() {
    let manager = Arc::new(SessionManager::new());
    let mut handles = vec![];

    // Create 1000 sessions concurrently
    for i in 0..1000 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            let conn = create_test_connection((20000 + i) % 65535, 443);
            let session_id = mgr
                .new_session(&format!("user{}", i % 100), conn, "allow", None)
                .await;

            // Random traffic
            for _ in 0..10 {
                mgr.update_traffic(&session_id, 100, 50, 1, 1).await;
            }

            mgr.close_session(&session_id, None, SessionStatus::Closed)
                .await;
        });
        handles.push(handle);
    }

    // Wait for all
    for handle in handles {
        handle.await.unwrap();
    }

    let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;
    assert_eq!(stats.active_sessions, 0);
    assert_eq!(stats.total_sessions, 1000);
}

#[tokio::test]
async fn test_ipv6_sessions() {
    let manager = SessionManager::new();

    let conn = ConnectionInfo {
        source_ip: IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)),
        source_port: 30000,
        dest_ip: "2001:db8::2".to_string(),
        dest_port: 443,
        protocol: rustsocks::session::types::Protocol::Tcp,
    };

    let session_id = manager.new_session("alice", conn, "allow", None).await;
    manager.update_traffic(&session_id, 1000, 500, 10, 5).await;
    manager
        .close_session(&session_id, None, SessionStatus::Closed)
        .await;

    let closed = manager.get_closed_sessions().await;
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].user.as_ref(), "alice");
}
