#[cfg(feature = "database")]
use super::batch::{BatchConfig, BatchWriter};
#[cfg(feature = "metrics")]
use super::metrics::SessionMetrics;
#[cfg(feature = "database")]
use super::store::SessionStore;
use super::types::{
    AclDecisionStats, ConnectionInfo, DestinationStat, Session, SessionStats,
    SessionStatus, UserSessionStat,
};
use chrono::{Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

/// In-memory session tracker built on top of DashMap.
#[derive(Debug)]
pub struct SessionManager {
    active_sessions: DashMap<Uuid, Arc<RwLock<Session>>>,
    closed_sessions: Mutex<Vec<Session>>,
    rejected_sessions: Mutex<Vec<Session>>,
    #[cfg(feature = "database")]
    store: Option<Arc<SessionStore>>,
    #[cfg(feature = "database")]
    batch_writer: Option<Arc<BatchWriter>>,
}

impl SessionManager {
    /// Create a new empty manager.
    pub fn new() -> Self {
        Self {
            active_sessions: DashMap::new(),
            closed_sessions: Mutex::new(Vec::new()),
            rejected_sessions: Mutex::new(Vec::new()),
            #[cfg(feature = "database")]
            store: None,
            #[cfg(feature = "database")]
            batch_writer: None,
        }
    }

    #[cfg(feature = "database")]
    pub fn with_store(store: Arc<SessionStore>, batch_config: BatchConfig) -> Self {
        let mut manager = Self::new();
        manager.set_store(store, batch_config);
        manager
    }

    #[cfg(feature = "database")]
    pub fn set_store(&mut self, store: Arc<SessionStore>, batch_config: BatchConfig) {
        let writer = BatchWriter::new(store.clone(), batch_config);
        writer.start();

        self.store = Some(store);
        self.batch_writer = Some(writer);
    }

    /// Start tracking a freshly accepted session.
    pub async fn new_session(
        &self,
        user: &str,
        connection: ConnectionInfo,
        acl_decision: impl Into<String>,
        acl_rule_matched: Option<String>,
    ) -> Uuid {
        let mut session =
            Session::new(user.to_string(), connection, acl_decision, acl_rule_matched);
        session.status = SessionStatus::Active;

        let session_id = session.session_id;

        #[cfg(feature = "metrics")]
        SessionMetrics::record_session_start(&session.user);

        #[cfg(feature = "database")]
        if let Some(writer) = &self.batch_writer {
            writer.enqueue(session.clone()).await;
        }

        self.active_sessions
            .insert(session_id, Arc::new(RwLock::new(session)));

        session_id
    }

    #[cfg(feature = "database")]
    pub async fn shutdown(&self) {
        if let Some(writer) = &self.batch_writer {
            writer.shutdown().await;
        }
    }

    /// Retrieve the active session handle if it exists.
    pub fn get_session(&self, session_id: &Uuid) -> Option<Arc<RwLock<Session>>> {
        self.active_sessions
            .get(session_id)
            .map(|guard| guard.value().clone())
    }

    /// Count currently active sessions.
    pub fn active_session_count(&self) -> usize {
        self.active_sessions.len()
    }

    /// Aggregate high-level statistics for sessions that started within the provided lookback window.
    pub async fn get_stats(&self, lookback: Duration) -> SessionStats {
        let now = Utc::now();
        let lookback_chrono =
            ChronoDuration::from_std(lookback).unwrap_or_else(|_| ChronoDuration::hours(24));
        let cutoff = now - lookback_chrono;

        let active_count = self.active_sessions.len();

        // Snapshot active sessions to avoid holding locks across await.
        let active_handles: Vec<_> = self
            .active_sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        let mut window_sessions: Vec<Session> = Vec::new();

        for handle in active_handles {
            let session = handle.read().await.clone();
            if session.start_time >= cutoff {
                window_sessions.push(session);
            }
        }

        let closed_sessions = self.closed_sessions.lock().unwrap().clone();
        window_sessions.extend(
            closed_sessions
                .into_iter()
                .filter(|session| session.start_time >= cutoff),
        );

        let rejected_sessions = self.rejected_sessions.lock().unwrap().clone();
        window_sessions.extend(
            rejected_sessions
                .into_iter()
                .filter(|session| session.start_time >= cutoff),
        );

        let total_sessions = window_sessions.len();
        let total_bytes = window_sessions.iter().fold(0u64, |acc, session| {
            acc.saturating_add(session.bytes_sent.saturating_add(session.bytes_received))
        });

        let mut user_counts: HashMap<String, u64> = HashMap::new();
        let mut destination_counts: HashMap<String, u64> = HashMap::new();
        let mut acl_allowed = 0u64;
        let mut acl_blocked = 0u64;

        for session in &window_sessions {
            *user_counts.entry(session.user.clone()).or_insert(0) += 1;
            *destination_counts
                .entry(session.dest_ip.clone())
                .or_insert(0) += 1;

            if session.acl_decision.eq_ignore_ascii_case("allow") {
                acl_allowed += 1;
            } else if session.acl_decision.eq_ignore_ascii_case("block") {
                acl_blocked += 1;
            }
        }

        const TOP_LIMIT: usize = 10;

        let mut top_users: Vec<UserSessionStat> = user_counts
            .into_iter()
            .map(|(user, sessions)| UserSessionStat { user, sessions })
            .collect();
        top_users.sort_by(|a, b| {
            b.sessions
                .cmp(&a.sessions)
                .then_with(|| a.user.cmp(&b.user))
        });
        top_users.truncate(TOP_LIMIT);

        let mut top_destinations: Vec<DestinationStat> = destination_counts
            .into_iter()
            .map(|(dest_ip, connections)| DestinationStat {
                dest_ip,
                connections,
            })
            .collect();
        top_destinations.sort_by(|a, b| {
            b.connections
                .cmp(&a.connections)
                .then_with(|| a.dest_ip.cmp(&b.dest_ip))
        });
        top_destinations.truncate(TOP_LIMIT);

        SessionStats {
            generated_at: now,
            active_sessions: active_count,
            total_sessions,
            total_bytes,
            top_users,
            top_destinations,
            acl: AclDecisionStats {
                allowed: acl_allowed,
                blocked: acl_blocked,
            },
        }
    }

    /// Update traffic counters for an active session.
    pub async fn update_traffic(
        &self,
        session_id: &Uuid,
        bytes_sent: u64,
        bytes_received: u64,
        packets_sent: u64,
        packets_received: u64,
    ) {
        if let Some(entry) = self.active_sessions.get(session_id) {
            let session = entry.value().clone();
            drop(entry);

            let mut session_guard = session.write().await;
            #[cfg(feature = "metrics")]
            let user_label = session_guard.user.clone();
            session_guard.bytes_sent = session_guard.bytes_sent.saturating_add(bytes_sent);
            session_guard.bytes_received =
                session_guard.bytes_received.saturating_add(bytes_received);
            session_guard.packets_sent = session_guard.packets_sent.saturating_add(packets_sent);
            session_guard.packets_received = session_guard
                .packets_received
                .saturating_add(packets_received);

            #[cfg(feature = "metrics")]
            SessionMetrics::record_traffic(&user_label, bytes_sent, bytes_received);

            #[cfg(feature = "database")]
            if let Some(writer) = &self.batch_writer {
                let snapshot = session_guard.clone();
                drop(session_guard);
                writer.enqueue(snapshot).await;
            }

            #[cfg(not(feature = "database"))]
            drop(session_guard);
        }
    }

    /// Close an active session and record it in the closed list.
    pub async fn close_session(
        &self,
        session_id: &Uuid,
        reason: Option<String>,
        status: SessionStatus,
    ) {
        if let Some((_, session_arc)) = self.active_sessions.remove(session_id) {
            let mut session = session_arc.write().await;
            session.close(reason, status);
            #[cfg(feature = "metrics")]
            SessionMetrics::record_session_close(session.duration_secs);
            let snapshot = session.clone();
            drop(session);

            self.closed_sessions.lock().unwrap().push(snapshot.clone());

            #[cfg(feature = "database")]
            if let Some(writer) = &self.batch_writer {
                writer.enqueue(snapshot).await;
            }
        }
    }

    /// Record a connection rejected before session creation (e.g., ACL block).
    pub async fn track_rejected_session(
        &self,
        user: &str,
        conn: ConnectionInfo,
        acl_rule: Option<String>,
    ) -> Uuid {
        let mut session = Session::new(
            user.to_string(),
            conn,
            "block",
            acl_rule,
        );

        session.close(
            Some("Rejected by ACL".to_string()),
            SessionStatus::RejectedByAcl,
        );

        #[cfg(feature = "metrics")]
        SessionMetrics::record_rejected_session(user);

        let session_id = session.session_id;

        #[cfg(feature = "database")]
        if let Some(writer) = &self.batch_writer {
            writer.enqueue(session.clone()).await;
        }

        self.rejected_sessions.lock().unwrap().push(session);

        session_id
    }

    /// Snapshot of all rejected sessions (testing/diagnostics).
    pub fn rejected_snapshot(&self) -> Vec<Session> {
        self.rejected_sessions.lock().unwrap().clone()
    }

    /// Snapshot of closed sessions (testing/diagnostics).
    pub fn closed_snapshot(&self) -> Vec<Session> {
        self.closed_sessions.lock().unwrap().clone()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::types::Protocol;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};

    #[cfg(feature = "metrics")]
    use crate::session::metrics::{
        REJECTED_SESSIONS, SESSION_DURATION, TOTAL_BYTES_RECEIVED, TOTAL_BYTES_SENT,
        TOTAL_SESSIONS, USER_BANDWIDTH, USER_SESSIONS,
    };
    #[cfg(feature = "metrics")]
    use lazy_static::lazy_static;

    #[cfg(feature = "metrics")]
    lazy_static! {
        static ref METRICS_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
    }

    fn sample_connection() -> ConnectionInfo {
        ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            source_port: 50000,
            dest_ip: "example.com".to_string(),
            dest_port: 443,
            protocol: Protocol::Tcp,
        }
    }

    #[tokio::test]
    async fn create_and_close_session() {
        let manager = SessionManager::new();
        let conn = sample_connection();
        let session_id = manager
            .new_session("alice", conn.clone(), "allow", Some("Allow HTTPS".into()))
            .await;

        assert_eq!(manager.active_session_count(), 1);
        manager.update_traffic(&session_id, 1024, 512, 10, 8).await;
        manager
            .close_session(
                &session_id,
                Some("Client disconnected".into()),
                SessionStatus::Closed,
            )
            .await;

        assert_eq!(manager.active_session_count(), 0);

        let closed = manager.closed_snapshot();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].user, "alice");
        assert_eq!(closed[0].bytes_sent, 1024);
        assert_eq!(closed[0].bytes_received, 512);
        assert_eq!(closed[0].status, SessionStatus::Closed);
        assert!(closed[0].end_time.is_some());
    }

    #[tokio::test]
    async fn track_rejected_session() {
        let manager = SessionManager::new();
        let conn = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            source_port: 40000,
            dest_ip: "blocked.example.com".into(),
            dest_port: 80,
            protocol: Protocol::Tcp,
        };
        let session_id = manager
            .track_rejected_session(
                "bob",
                conn,
                Some("Block admin".into()),
            )
            .await;

        assert_ne!(session_id, Uuid::nil());
        assert_eq!(manager.active_session_count(), 0);
        let rejected = manager.rejected_snapshot();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].status, SessionStatus::RejectedByAcl);
        assert_eq!(rejected[0].acl_decision, "block");
        assert_eq!(rejected[0].acl_rule_matched.as_deref(), Some("Block admin"));
    }

    #[tokio::test]
    async fn get_stats_aggregates_today_sessions() {
        let manager = SessionManager::new();

        let mut conn_a = sample_connection();
        conn_a.dest_ip = "app.internal".into();

        let session_a = manager
            .new_session("alice", conn_a, "allow", Some("Allow app".into()))
            .await;
        manager.update_traffic(&session_a, 150, 50, 2, 1).await;
        manager
            .close_session(&session_a, Some("finished".into()), SessionStatus::Closed)
            .await;

        let mut conn_b = sample_connection();
        conn_b.dest_ip = "api.internal".into();
        let session_b = manager.new_session("bob", conn_b, "allow", None).await;
        if let Some(handle) = manager.get_session(&session_b) {
            let mut guard = handle.write().await;
            guard.start_time = guard.start_time - ChronoDuration::hours(48);
        }

        let conn_carol = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            source_port: 41000,
            dest_ip: "blocked.internal".into(),
            dest_port: 8080,
            protocol: Protocol::Tcp,
        };
        manager
            .track_rejected_session(
                "carol",
                conn_carol,
                Some("Block admin".into()),
            )
            .await;

        let stats = manager.get_stats(Duration::from_secs(24 * 3600)).await;

        assert_eq!(stats.active_sessions, 1);
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.total_bytes, 200);

        let users: HashMap<_, _> = stats
            .top_users
            .iter()
            .map(|entry| (entry.user.as_str(), entry.sessions))
            .collect();
        assert_eq!(users.get("alice"), Some(&1));
        assert_eq!(users.get("carol"), Some(&1));
        assert!(users.get("bob").is_none());

        let destinations: HashMap<_, _> = stats
            .top_destinations
            .iter()
            .map(|entry| (entry.dest_ip.as_str(), entry.connections))
            .collect();
        assert_eq!(destinations.get("app.internal"), Some(&1));
        assert_eq!(destinations.get("blocked.internal"), Some(&1));
        assert!(destinations.get("api.internal").is_none());

        assert_eq!(stats.acl.allowed, 1);
        assert_eq!(stats.acl.blocked, 1);
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn session_metrics_update_counters() {
        let _guard = METRICS_TEST_GUARD.lock().unwrap();

        let base_total = TOTAL_SESSIONS.get();
        let base_rejected = REJECTED_SESSIONS.get();
        let base_bytes_sent = TOTAL_BYTES_SENT.get();
        let base_bytes_received = TOTAL_BYTES_RECEIVED.get();
        let base_duration_count = SESSION_DURATION.get_sample_count();
        let base_user_alice = USER_SESSIONS.with_label_values(&["alice"]).get();
        let base_user_bob = USER_SESSIONS.with_label_values(&["bob"]).get();
        let base_user_send = USER_BANDWIDTH.with_label_values(&["alice", "sent"]).get();
        let base_user_recv = USER_BANDWIDTH
            .with_label_values(&["alice", "received"])
            .get();

        let manager = SessionManager::new();
        let conn = sample_connection();
        let session_id = manager.new_session("alice", conn, "allow", None).await;

        assert!(
            TOTAL_SESSIONS.get() >= base_total + 1,
            "total sessions should increase"
        );
        assert!(
            USER_SESSIONS.with_label_values(&["alice"]).get() >= base_user_alice + 1,
            "user sessions counter should increase"
        );

        manager.update_traffic(&session_id, 512, 256, 2, 2).await;

        assert!(
            TOTAL_BYTES_SENT.get() >= base_bytes_sent + 512,
            "bytes sent counter should increase"
        );
        assert!(
            TOTAL_BYTES_RECEIVED.get() >= base_bytes_received + 256,
            "bytes received counter should increase"
        );
        assert!(
            USER_BANDWIDTH.with_label_values(&["alice", "sent"]).get() >= base_user_send + 512,
            "user sent bandwidth should increase"
        );
        assert!(
            USER_BANDWIDTH
                .with_label_values(&["alice", "received"])
                .get()
                >= base_user_recv + 256,
            "user received bandwidth should increase"
        );

        manager
            .close_session(
                &session_id,
                Some("metrics close".into()),
                SessionStatus::Closed,
            )
            .await;

        assert!(
            SESSION_DURATION.get_sample_count() >= base_duration_count + 1,
            "duration histogram should record the session"
        );

        let conn_rejected = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            source_port: 40001,
            dest_ip: "blocked.metrics.example".into(),
            dest_port: 1080,
            protocol: Protocol::Tcp,
        };
        manager
            .track_rejected_session(
                "bob",
                conn_rejected,
                None,
            )
            .await;

        assert!(
            REJECTED_SESSIONS.get() >= base_rejected + 1,
            "rejected sessions counter should increase"
        );
        assert!(
            USER_SESSIONS.with_label_values(&["bob"]).get() >= base_user_bob + 1,
            "user sessions counter should track rejected users"
        );
    }
}
