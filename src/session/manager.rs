use super::types::{ConnectionInfo, Protocol, Session, SessionStatus};
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use uuid::Uuid;

/// In-memory session tracker built on top of DashMap.
#[derive(Debug)]
pub struct SessionManager {
    active_sessions: DashMap<Uuid, Arc<RwLock<Session>>>,
    closed_sessions: Mutex<Vec<Session>>,
    rejected_sessions: Mutex<Vec<Session>>,
}

impl SessionManager {
    /// Create a new empty manager.
    pub fn new() -> Self {
        Self {
            active_sessions: DashMap::new(),
            closed_sessions: Mutex::new(Vec::new()),
            rejected_sessions: Mutex::new(Vec::new()),
        }
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
        self.active_sessions
            .insert(session_id, Arc::new(RwLock::new(session)));

        session_id
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
            session_guard.bytes_sent = session_guard.bytes_sent.saturating_add(bytes_sent);
            session_guard.bytes_received =
                session_guard.bytes_received.saturating_add(bytes_received);
            session_guard.packets_sent = session_guard.packets_sent.saturating_add(packets_sent);
            session_guard.packets_received = session_guard
                .packets_received
                .saturating_add(packets_received);
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
            let snapshot = session.clone();
            drop(session);

            self.closed_sessions.lock().unwrap().push(snapshot);
        }
    }

    /// Record a connection rejected before session creation (e.g., ACL block).
    pub fn track_rejected_session(
        &self,
        user: &str,
        source_ip: IpAddr,
        source_port: u16,
        dest_ip: String,
        dest_port: u16,
        protocol: Protocol,
        acl_rule: Option<String>,
    ) -> Uuid {
        let mut session = Session::new(
            user.to_string(),
            ConnectionInfo {
                source_ip,
                source_port,
                dest_ip,
                dest_port,
                protocol,
            },
            "block",
            acl_rule,
        );

        session.close(
            Some("Rejected by ACL".to_string()),
            SessionStatus::RejectedByAcl,
        );

        let session_id = session.session_id;

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
    use std::net::{IpAddr, Ipv4Addr};

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
        let session_id = manager.track_rejected_session(
            "bob",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40000,
            "blocked.example.com".into(),
            80,
            Protocol::Tcp,
            Some("Block admin".into()),
        );

        assert_ne!(session_id, Uuid::nil());
        assert_eq!(manager.active_session_count(), 0);
        let rejected = manager.rejected_snapshot();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].status, SessionStatus::RejectedByAcl);
        assert_eq!(rejected[0].acl_decision, "block");
        assert_eq!(rejected[0].acl_rule_matched.as_deref(), Some("Block admin"));
    }
}
