#[cfg(feature = "database")]
use super::batch::{BatchConfig, BatchWriter};
#[cfg(feature = "metrics")]
use super::metrics::SessionMetrics;
#[cfg(feature = "database")]
use super::store::SessionStore;
use super::types::{
    AclDecisionStats, ConnectionInfo, DestinationStat, Session, SessionStats, SessionStatus,
    UserSessionStat,
};
use crate::acl::{AclDecision, AclEngine, Protocol as AclProtocol};
use crate::protocol::Address;
use chrono::{Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
#[cfg(feature = "database")]
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::{
    broadcast,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    RwLock,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

/// In-memory session tracker built on top of DashMap.
///
/// Optimizations:
/// - Uses RwLock instead of Mutex for closed/rejected sessions (allows concurrent reads in get_stats)
/// - DashMap for active sessions (lock-free concurrent access)
#[derive(Debug)]
pub struct SessionManager {
    active_sessions: DashMap<Uuid, Arc<RwLock<Session>>>,
    closed_sessions: RwLock<Vec<Session>>,
    rejected_sessions: RwLock<Vec<Session>>,
    session_controls: DashMap<Uuid, SessionControl>,
    #[cfg(feature = "database")]
    store: Option<Arc<SessionStore>>,
    #[cfg(feature = "database")]
    batch_writer: OnceLock<Arc<BatchWriter>>,
    traffic_tx: UnboundedSender<TrafficUpdate>,
}

#[derive(Debug, Clone)]
struct SessionControl {
    cancel_token: CancellationToken,
    udp_shutdown: Option<broadcast::Sender<()>>,
}

#[derive(Debug, Clone, Copy)]
struct TrafficUpdate {
    session_id: Uuid,
    bytes_sent: u64,
    bytes_received: u64,
    packets_sent: u64,
    packets_received: u64,
}

impl SessionManager {
    /// Create a new empty manager.
    pub fn new() -> Self {
        let (traffic_tx, traffic_rx) = unbounded_channel();
        let manager = Self {
            active_sessions: DashMap::new(),
            closed_sessions: RwLock::new(Vec::new()),
            rejected_sessions: RwLock::new(Vec::new()),
            session_controls: DashMap::new(),
            #[cfg(feature = "database")]
            store: None,
            #[cfg(feature = "database")]
            batch_writer: OnceLock::new(),
            traffic_tx,
        };

        manager.start_traffic_worker(traffic_rx);
        manager
    }

    fn start_traffic_worker(&self, mut rx: UnboundedReceiver<TrafficUpdate>) {
        let active_sessions = self.active_sessions.clone();
        #[cfg(feature = "database")]
        let batch_writer = self.batch_writer.clone();

        tokio::spawn(async move {
            while let Some(update) = rx.recv().await {
                SessionManager::apply_traffic_update(
                    &active_sessions,
                    #[cfg(feature = "database")]
                    &batch_writer,
                    update,
                )
                .await;
            }
        });
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
        // OnceLock::set returns Err if already set, which is OK - we only set once
        let _ = self.batch_writer.set(writer);
    }

    /// Start tracking a freshly accepted session.
    pub async fn new_session(
        &self,
        user: &str,
        connection: ConnectionInfo,
        acl_decision: impl Into<String>,
        acl_rule_matched: Option<String>,
    ) -> Uuid {
        self.new_session_with_control(user, connection, acl_decision, acl_rule_matched, None)
            .await
            .0
    }

    /// Start tracking a session and return its cancellation token so callers can react to shutdown.
    pub async fn new_session_with_control(
        &self,
        user: &str,
        connection: ConnectionInfo,
        acl_decision: impl Into<String>,
        acl_rule_matched: Option<String>,
        udp_shutdown: Option<broadcast::Sender<()>>,
    ) -> (Uuid, CancellationToken) {
        let mut session =
            Session::new(user.to_string(), connection, acl_decision, acl_rule_matched);
        session.status = SessionStatus::Active;

        let session_id = session.session_id;
        let cancel_token = CancellationToken::new();

        self.session_controls.insert(
            session_id,
            SessionControl {
                cancel_token: cancel_token.clone(),
                udp_shutdown,
            },
        );

        #[cfg(feature = "metrics")]
        SessionMetrics::record_session_start(&session.user);

        #[cfg(feature = "database")]
        if let Some(writer) = self.current_batch_writer() {
            writer.enqueue(session.clone()).await;
        }

        self.active_sessions
            .insert(session_id, Arc::new(RwLock::new(session)));

        (session_id, cancel_token)
    }

    #[cfg(feature = "database")]
    pub async fn shutdown(&self) {
        self.close_all_active("Server shutdown", SessionStatus::Failed)
            .await;

        #[cfg(feature = "database")]
        if let Some(writer) = self.current_batch_writer() {
            writer.shutdown().await;
        }
    }

    #[cfg(feature = "database")]
    fn current_batch_writer(&self) -> Option<Arc<BatchWriter>> {
        self.batch_writer.get().cloned()
    }

    #[cfg(feature = "database")]
    fn clone_batch_writer_handle(handle: &OnceLock<Arc<BatchWriter>>) -> Option<Arc<BatchWriter>> {
        handle.get().cloned()
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
    /// Optimized to aggregate data during iteration instead of collecting all sessions first.
    pub async fn get_stats(&self, lookback: Duration) -> SessionStats {
        let now = Utc::now();
        let lookback_chrono =
            ChronoDuration::from_std(lookback).unwrap_or_else(|_| ChronoDuration::hours(24));
        let cutoff = now - lookback_chrono;

        let active_count = self.active_sessions.len();

        // Pre-allocate aggregation maps with reasonable capacity
        let mut user_counts: HashMap<String, u64> = HashMap::with_capacity(100);
        let mut destination_counts: HashMap<String, u64> = HashMap::with_capacity(100);
        let mut acl_allowed = 0u64;
        let mut acl_blocked = 0u64;
        let mut total_sessions = 0usize;
        let mut total_bytes = 0u64;

        // Helper closure to aggregate a single session (avoids code duplication)
        let mut aggregate_session = |user: &str,
                                     dest_ip: &str,
                                     bytes_sent: u64,
                                     bytes_received: u64,
                                     acl_decision: &str| {
            *user_counts.entry(user.to_string()).or_insert(0) += 1;
            *destination_counts.entry(dest_ip.to_string()).or_insert(0) += 1;
            total_bytes = total_bytes.saturating_add(bytes_sent.saturating_add(bytes_received));
            total_sessions += 1;

            if acl_decision.eq_ignore_ascii_case("allow") {
                acl_allowed += 1;
            } else if acl_decision.eq_ignore_ascii_case("block") {
                acl_blocked += 1;
            }
        };

        // Snapshot active sessions to avoid holding locks across await.
        let active_handles: Vec<_> = self
            .active_sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        // Process active sessions - aggregate data directly without intermediate Vec
        for handle in active_handles {
            let session = handle.read().await;
            if session.start_time >= cutoff {
                aggregate_session(
                    &session.user,
                    &session.dest_ip,
                    session.bytes_sent,
                    session.bytes_received,
                    &session.acl_decision,
                );
            }
        }

        // Process closed sessions - iterate without cloning entire Vec
        // Using RwLock.read() allows concurrent reads without blocking
        {
            let closed = self.closed_sessions.read().await;
            for session in closed.iter().filter(|s| s.start_time >= cutoff) {
                aggregate_session(
                    &session.user,
                    &session.dest_ip,
                    session.bytes_sent,
                    session.bytes_received,
                    &session.acl_decision,
                );
            }
        }

        // Process rejected sessions - iterate without cloning entire Vec
        // Using RwLock.read() allows concurrent reads without blocking
        {
            let rejected = self.rejected_sessions.read().await;
            for session in rejected.iter().filter(|s| s.start_time >= cutoff) {
                aggregate_session(
                    &session.user,
                    &session.dest_ip,
                    session.bytes_sent,
                    session.bytes_received,
                    &session.acl_decision,
                );
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
        Self::apply_traffic_update(
            &self.active_sessions,
            #[cfg(feature = "database")]
            &self.batch_writer,
            TrafficUpdate {
                session_id: *session_id,
                bytes_sent,
                bytes_received,
                packets_sent,
                packets_received,
            },
        )
        .await;
    }

    pub fn queue_traffic_update(
        &self,
        session_id: &Uuid,
        bytes_sent: u64,
        bytes_received: u64,
        packets_sent: u64,
        packets_received: u64,
    ) {
        let update = TrafficUpdate {
            session_id: *session_id,
            bytes_sent,
            bytes_received,
            packets_sent,
            packets_received,
        };

        if let Err(err) = self.traffic_tx.send(update) {
            warn!(
                session = %session_id,
                "Failed to enqueue traffic update: {}",
                err
            );
        }
    }

    async fn apply_traffic_update(
        active_sessions: &DashMap<Uuid, Arc<RwLock<Session>>>,
        #[cfg(feature = "database")] batch_writer: &OnceLock<Arc<BatchWriter>>,
        update: TrafficUpdate,
    ) {
        if let Some(entry) = active_sessions.get(&update.session_id) {
            let session = entry.value().clone();
            drop(entry);

            let mut session_guard = session.write().await;
            #[cfg(feature = "metrics")]
            let user_label = session_guard.user.clone();
            session_guard.bytes_sent = session_guard.bytes_sent.saturating_add(update.bytes_sent);
            session_guard.bytes_received = session_guard
                .bytes_received
                .saturating_add(update.bytes_received);
            session_guard.packets_sent = session_guard
                .packets_sent
                .saturating_add(update.packets_sent);
            session_guard.packets_received = session_guard
                .packets_received
                .saturating_add(update.packets_received);

            #[cfg(feature = "metrics")]
            SessionMetrics::record_traffic(&user_label, update.bytes_sent, update.bytes_received);

            #[cfg(feature = "database")]
            if let Some(writer) = Self::clone_batch_writer_handle(batch_writer) {
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
        self.session_controls.remove(session_id);

        if let Some((_, session_arc)) = self.active_sessions.remove(session_id) {
            let mut session = session_arc.write().await;
            session.close(reason, status);
            #[cfg(feature = "metrics")]
            SessionMetrics::record_session_close(session.duration_secs);
            let snapshot = session.clone();
            drop(session);

            // Use write lock for appending to closed sessions
            // RwLock reduces contention compared to Mutex for read-heavy workloads
            self.closed_sessions.write().await.push(snapshot.clone());

            #[cfg(feature = "database")]
            if let Some(writer) = self.current_batch_writer() {
                writer.enqueue(snapshot).await;
            }
        }
    }

    /// Terminate an active session by cancelling underlying IO and recording closure.
    pub async fn terminate_session(
        &self,
        session_id: &Uuid,
        reason: impl Into<String>,
        status: SessionStatus,
    ) {
        if let Some(control) = self.session_controls.get(session_id) {
            control.cancel_token.cancel();
            if let Some(tx) = &control.udp_shutdown {
                let _ = tx.send(());
            }
        }

        self.close_session(session_id, Some(reason.into()), status)
            .await;
    }

    /// Record a connection rejected before session creation (e.g., ACL block).
    pub async fn track_rejected_session(
        &self,
        user: &str,
        conn: ConnectionInfo,
        acl_rule: Option<String>,
    ) -> Uuid {
        let mut session = Session::new(user.to_string(), conn, "block", acl_rule);

        session.close(
            Some("Rejected by ACL".to_string()),
            SessionStatus::RejectedByAcl,
        );

        #[cfg(feature = "metrics")]
        SessionMetrics::record_rejected_session(user);

        let session_id = session.session_id;

        #[cfg(feature = "database")]
        if let Some(writer) = self.current_batch_writer() {
            writer.enqueue(session.clone()).await;
        }

        // Use write lock for appending to rejected sessions
        self.rejected_sessions.write().await.push(session);

        session_id
    }

    /// Snapshot of all rejected sessions (testing/diagnostics).
    pub async fn rejected_snapshot(&self) -> Vec<Session> {
        self.rejected_sessions.read().await.clone()
    }

    /// Snapshot of closed sessions (testing/diagnostics).
    pub async fn closed_snapshot(&self) -> Vec<Session> {
        self.closed_sessions.read().await.clone()
    }

    /// Close all active sessions with a common reason/status (e.g., server shutdown).
    pub async fn close_all_active(&self, reason: &str, status: SessionStatus) {
        let reason = reason.to_string();
        let session_ids: Vec<Uuid> = self
            .active_sessions
            .iter()
            .map(|entry| *entry.key())
            .collect();

        for session_id in session_ids {
            self.terminate_session(&session_id, reason.clone(), status.clone())
                .await;
        }
    }

    /// Re-evaluate all active sessions against the provided ACL engine and terminate those that are no longer allowed.
    pub async fn enforce_acl(&self, acl_engine: Arc<AclEngine>) {
        let mut to_terminate = Vec::new();

        for entry in self.active_sessions.iter() {
            let session_guard = entry.value().read().await;
            let session = session_guard.clone();
            drop(session_guard);

            let address = if let Ok(ipv4) = session.dest_ip.parse::<Ipv4Addr>() {
                Address::IPv4(ipv4.octets())
            } else if let Ok(ipv6) = session.dest_ip.parse::<Ipv6Addr>() {
                Address::IPv6(ipv6.octets())
            } else {
                Address::Domain(session.dest_ip.to_string())
            };

            let acl_protocol = match session.protocol {
                super::types::Protocol::Tcp => AclProtocol::Tcp,
                super::types::Protocol::Udp => AclProtocol::Udp,
            };

            let (decision, matched_rule) = acl_engine
                .evaluate(&session.user, &address, session.dest_port, &acl_protocol)
                .await;

            if decision == AclDecision::Block {
                let rule_desc = matched_rule.unwrap_or_else(|| "Default policy".to_string());
                let reason = format!("Terminated by ACL update ({})", rule_desc);
                to_terminate.push((
                    session.session_id,
                    reason,
                    session.user.clone(),
                    session.dest_ip.clone(),
                    session.dest_port,
                ));
            }
        }

        if !to_terminate.is_empty() {
            info!(
                count = to_terminate.len(),
                "Revoking sessions due to ACL update"
            );
        }

        for (session_id, reason, user, dest, port) in to_terminate {
            warn!(
                %session_id,
                user = %user,
                dest = %dest,
                port,
                "Closing session after ACL update"
            );
            self.terminate_session(&session_id, reason.clone(), SessionStatus::Failed)
                .await;
        }
    }

    /// Get all sessions (active + closed + rejected)
    pub async fn get_all_sessions(&self) -> Vec<Session> {
        let mut all = Vec::new();

        // Add active sessions
        for entry in self.active_sessions.iter() {
            let session = entry.value().read().await.clone();
            all.push(session);
        }

        // Add closed sessions
        all.extend(self.closed_sessions.read().await.clone());

        // Add rejected sessions
        all.extend(self.rejected_sessions.read().await.clone());

        all
    }

    /// Get active sessions only
    pub async fn get_active_sessions(&self) -> Vec<Session> {
        let mut sessions = Vec::new();
        for entry in self.active_sessions.iter() {
            let session = entry.value().read().await.clone();
            sessions.push(session);
        }
        sessions
    }

    /// Get closed sessions only
    pub async fn get_closed_sessions(&self) -> Vec<Session> {
        self.closed_sessions.read().await.clone()
    }

    #[cfg(feature = "database")]
    pub fn session_store(&self) -> Option<Arc<SessionStore>> {
        self.store.as_ref().map(Arc::clone)
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
    use crate::acl::types::{
        AclConfig, AclRule, Action, GlobalAclConfig, Protocol as AclAclProtocol, UserAcl,
    };
    use crate::acl::AclEngine;
    use crate::session::types::Protocol;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;

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

        let closed = manager.closed_snapshot().await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].user.as_ref(), "alice");
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
            .track_rejected_session("bob", conn, Some("Block admin".into()))
            .await;

        assert_ne!(session_id, Uuid::nil());
        assert_eq!(manager.active_session_count(), 0);
        let rejected = manager.rejected_snapshot().await;
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].status, SessionStatus::RejectedByAcl);
        assert_eq!(rejected[0].acl_decision.as_ref(), "block");
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
            guard.start_time -= ChronoDuration::hours(48);
        }

        let conn_carol = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            source_port: 41000,
            dest_ip: "blocked.internal".into(),
            dest_port: 8080,
            protocol: Protocol::Tcp,
        };
        manager
            .track_rejected_session("carol", conn_carol, Some("Block admin".into()))
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
        assert!(!users.contains_key("bob"));

        let destinations: HashMap<_, _> = stats
            .top_destinations
            .iter()
            .map(|entry| (entry.dest_ip.as_str(), entry.connections))
            .collect();
        assert_eq!(destinations.get("app.internal"), Some(&1));
        assert_eq!(destinations.get("blocked.internal"), Some(&1));
        assert!(!destinations.contains_key("api.internal"));

        assert_eq!(stats.acl.allowed, 1);
        assert_eq!(stats.acl.blocked, 1);
    }

    #[tokio::test]
    async fn close_all_active_marks_sessions_closed() {
        let manager = SessionManager::new();
        let conn = sample_connection();

        let session_id = manager
            .new_session("alice", conn.clone(), "allow", None)
            .await;

        assert_eq!(manager.active_session_count(), 1);

        manager
            .close_all_active("Server shutdown", SessionStatus::Failed)
            .await;

        assert_eq!(manager.active_session_count(), 0);

        let closed = manager.closed_snapshot().await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].session_id, session_id);
        assert_eq!(closed[0].status, SessionStatus::Failed);
        assert_eq!(closed[0].close_reason.as_deref(), Some("Server shutdown"));
        assert!(closed[0].end_time.is_some());
        assert!(closed[0].duration_secs.is_some());
    }

    #[tokio::test]
    async fn enforce_acl_revokes_blocked_session() {
        let manager = SessionManager::new();
        let mut conn = sample_connection();
        conn.dest_ip = "10.42.0.10".into();
        conn.dest_port = 443;

        let initial_config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Allow,
            },
            users: vec![UserAcl {
                username: "alice".into(),
                groups: vec![],
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Allow all".into(),
                    destinations: vec!["0.0.0.0/0".into()],
                    ports: vec!["*".into()],
                    protocols: vec![AclAclProtocol::Tcp],
                    priority: 10,
                }],
            }],
            groups: vec![],
        };

        let engine = Arc::new(AclEngine::new(initial_config).expect("engine"));

        let (_session_id, _token) = manager
            .new_session_with_control("alice", conn.clone(), "allow", None, None)
            .await;

        assert_eq!(manager.active_session_count(), 1);

        let block_config = AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".into(),
                groups: vec![],
                rules: vec![AclRule {
                    action: Action::Block,
                    description: "Block test dest".into(),
                    destinations: vec!["10.42.0.10".into()],
                    ports: vec!["443".into()],
                    protocols: vec![AclAclProtocol::Tcp],
                    priority: 500,
                }],
            }],
            groups: vec![],
        };

        engine.reload(block_config).await.expect("reload");

        manager.enforce_acl(engine.clone()).await;

        assert_eq!(manager.active_session_count(), 0);
        let closed = manager.closed_snapshot().await;
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].status, SessionStatus::Failed);
        assert_eq!(
            closed[0].close_reason.as_deref(),
            Some("Terminated by ACL update (Block test dest)"),
        );
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn session_metrics_update_counters() {
        let (
            base_total,
            base_rejected,
            base_bytes_sent,
            base_bytes_received,
            base_duration_count,
            base_user_alice,
            base_user_bob,
            base_user_send,
            base_user_recv,
        ) = {
            let _guard = METRICS_TEST_GUARD.lock().unwrap();
            (
                TOTAL_SESSIONS.get(),
                REJECTED_SESSIONS.get(),
                TOTAL_BYTES_SENT.get(),
                TOTAL_BYTES_RECEIVED.get(),
                SESSION_DURATION.get_sample_count(),
                USER_SESSIONS.with_label_values(&["alice"]).get(),
                USER_SESSIONS.with_label_values(&["bob"]).get(),
                USER_BANDWIDTH.with_label_values(&["alice", "sent"]).get(),
                USER_BANDWIDTH
                    .with_label_values(&["alice", "received"])
                    .get(),
            )
        };

        let manager = SessionManager::new();
        let conn = sample_connection();
        let session_id = manager.new_session("alice", conn, "allow", None).await;

        assert!(
            TOTAL_SESSIONS.get() > base_total,
            "total sessions should increase"
        );
        assert!(
            USER_SESSIONS.with_label_values(&["alice"]).get() > base_user_alice,
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
            SESSION_DURATION.get_sample_count() > base_duration_count,
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
            .track_rejected_session("bob", conn_rejected, None)
            .await;

        assert!(
            REJECTED_SESSIONS.get() > base_rejected,
            "rejected sessions counter should increase"
        );
        assert!(
            USER_SESSIONS.with_label_values(&["bob"]).get() > base_user_bob,
            "user sessions counter should track rejected users"
        );
    }
}
