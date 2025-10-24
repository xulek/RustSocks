use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::IpAddr;
use uuid::Uuid;

/// Transport protocol associated with a session.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Tcp => write!(f, "tcp"),
            Protocol::Udp => write!(f, "udp"),
        }
    }
}

/// Lifecycle state of a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Closed,
    Failed,
    RejectedByAcl,
}

/// Core representation of a SOCKS session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    // Identity
    pub session_id: Uuid,
    pub user: String,

    // Timing
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_secs: Option<u64>,

    // Network
    pub source_ip: IpAddr,
    pub source_port: u16,
    pub dest_ip: String,
    pub dest_port: u16,
    pub protocol: Protocol,

    // Traffic stats
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,

    // Status
    pub status: SessionStatus,
    pub close_reason: Option<String>,

    // ACL
    pub acl_rule_matched: Option<String>,
    pub acl_decision: String,
}

impl Session {
    /// Create a new active session from connection info.
    pub fn new(
        user: impl Into<String>,
        connection: ConnectionInfo,
        acl_decision: impl Into<String>,
        acl_rule_matched: Option<String>,
    ) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            user: user.into(),
            start_time: Utc::now(),
            end_time: None,
            duration_secs: None,
            source_ip: connection.source_ip,
            source_port: connection.source_port,
            dest_ip: connection.dest_ip,
            dest_port: connection.dest_port,
            protocol: connection.protocol,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            status: SessionStatus::Active,
            close_reason: None,
            acl_rule_matched,
            acl_decision: acl_decision.into(),
        }
    }

    /// Mark the session as closed and compute duration.
    pub fn close(&mut self, reason: Option<String>, status: SessionStatus) {
        self.end_time = Some(Utc::now());
        if let Some(end) = self.end_time {
            self.duration_secs = Some((end - self.start_time).num_seconds().max(0) as u64);
        }
        self.status = status;
        self.close_reason = reason;
    }
}

/// Immutable connection metadata collected at session start.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub source_ip: IpAddr,
    pub source_port: u16,
    pub dest_ip: String,
    pub dest_port: u16,
    pub protocol: Protocol,
}

/// User-provided filters for querying session history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFilter {
    pub user: Option<String>,
    pub status: Option<SessionStatus>,
    pub start_after: Option<DateTime<Utc>>,
    pub start_before: Option<DateTime<Utc>>,
    pub dest_ip: Option<String>,
    pub min_duration_secs: Option<u64>,
    pub min_bytes: Option<u64>,
    pub limit: Option<u64>,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self {
            user: None,
            status: None,
            start_after: None,
            start_before: None,
            dest_ip: None,
            min_duration_secs: None,
            min_bytes: None,
            limit: Some(100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn protocol_display_matches_lowercase() {
        assert_eq!(Protocol::Tcp.to_string(), "tcp");
        assert_eq!(Protocol::Udp.to_string(), "udp");
    }

    #[test]
    fn session_filter_has_default_limit() {
        let filter = SessionFilter::default();
        assert_eq!(filter.limit, Some(100));
        assert!(filter.user.is_none());
        assert!(filter.status.is_none());
    }

    #[test]
    fn session_status_serializes_to_snake_case() {
        let value = serde_json::to_string(&SessionStatus::RejectedByAcl).unwrap();
        assert_eq!(value, "\"rejected_by_acl\"");
    }

    #[test]
    fn session_serializes_with_acl_fields() {
        let connection = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            source_port: 12345,
            dest_ip: "example.com".to_string(),
            dest_port: 443,
            protocol: Protocol::Tcp,
        };

        let session = Session::new("alice", connection, "allow", Some("Allow HTTPS".into()));
        let json_value: Value = serde_json::to_value(&session).unwrap();
        assert_eq!(json_value["status"], json!("active"));
        assert_eq!(json_value["acl_decision"], json!("allow"));
        assert!(json_value["acl_rule_matched"].is_string());
    }
}
