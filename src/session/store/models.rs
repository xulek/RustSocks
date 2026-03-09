use super::sqlx;
use crate::session::history::MetricsSnapshot;
use crate::session::types::{Protocol as SessionProtocol, Session, SessionStatus};
use crate::smtp::encryption::decrypt_password;
use crate::smtp::types::{parse_recipients, SmtpConfig, SmtpMode};
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx_core::row::Row as SqlxRow;
use sqlx_sqlite::SqliteRow;
use std::borrow::Cow;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct MetricSnapshotRow {
    pub(super) timestamp: String,
    pub(super) active_sessions: i64,
    pub(super) total_sessions: i64,
    pub(super) bandwidth: i64,
}

impl MetricSnapshotRow {
    pub(super) fn into_metric(self) -> Result<MetricsSnapshot, sqlx::Error> {
        let timestamp = parse_datetime("timestamp", &self.timestamp)?;

        Ok(MetricsSnapshot {
            timestamp,
            active_sessions: self.active_sessions as u64,
            total_sessions: self.total_sessions as u64,
            bandwidth: self.bandwidth as u64,
        })
    }
}

#[derive(Debug)]
pub(super) struct SmtpConfigRow {
    pub(super) enabled: i32,
    pub(super) mode: String,
    pub(super) host: String,
    pub(super) port: i32,
    pub(super) from_address: String,
    pub(super) from_name: Option<String>,
    pub(super) username: Option<String>,
    pub(super) password_encrypted: Option<String>,
    pub(super) notify_recipients: Option<String>,
    pub(super) notify_critical: i32,
    pub(super) notify_security: i32,
    pub(super) notify_config_changes: i32,
    pub(super) notify_service_status: i32,
    pub(super) notify_resource_pressure: i32,
    pub(super) notify_connection_pressure: i32,
    pub(super) notify_cooldown_seconds: i32,
    pub(super) notify_cpu_threshold: i32,
    pub(super) notify_ram_threshold: i32,
    pub(super) notify_disk_threshold: i32,
    pub(super) notify_connection_percent_threshold: i32,
}

impl SmtpConfigRow {
    pub(super) fn into_smtp_config(self, api_token: Option<&str>) -> Result<SmtpConfig, String> {
        let mode: SmtpMode = self.mode.parse().unwrap_or_default();
        let has_password = self.password_encrypted.is_some();
        let notify_recipients = parse_recipients(self.notify_recipients.as_deref().unwrap_or(""));
        let password = match (&self.password_encrypted, api_token) {
            (Some(enc), Some(token)) => decrypt_password(enc, token).ok(),
            _ => None,
        };

        Ok(SmtpConfig {
            enabled: self.enabled != 0,
            mode,
            host: self.host,
            port: self.port as u16,
            from_address: self.from_address,
            from_name: self.from_name,
            username: self.username,
            password,
            has_password,
            notify_recipients,
            notify_critical: self.notify_critical != 0,
            notify_security: self.notify_security != 0,
            notify_config_changes: self.notify_config_changes != 0,
            notify_service_status: self.notify_service_status != 0,
            notify_resource_pressure: self.notify_resource_pressure != 0,
            notify_connection_pressure: self.notify_connection_pressure != 0,
            notify_cooldown_seconds: self.notify_cooldown_seconds.max(0) as u64,
            notify_cpu_threshold: self.notify_cpu_threshold.clamp(1, 100) as u8,
            notify_ram_threshold: self.notify_ram_threshold.clamp(1, 100) as u8,
            notify_disk_threshold: self.notify_disk_threshold.clamp(1, 100) as u8,
            notify_connection_percent_threshold: self
                .notify_connection_percent_threshold
                .clamp(1, 100) as u8,
        })
    }
}

#[derive(Debug)]
pub(super) struct SessionRow {
    pub(super) session_id: String,
    pub(super) user: String,
    pub(super) start_time: String,
    pub(super) end_time: Option<String>,
    pub(super) duration_secs: Option<i64>,
    pub(super) source_ip: String,
    pub(super) source_port: i64,
    pub(super) dest_ip: String,
    pub(super) dest_port: i64,
    pub(super) protocol: String,
    pub(super) bytes_sent: i64,
    pub(super) bytes_received: i64,
    pub(super) packets_sent: i64,
    pub(super) packets_received: i64,
    pub(super) status: String,
    pub(super) close_reason: Option<String>,
    pub(super) acl_rule_matched: Option<String>,
    pub(super) acl_decision: String,
}

#[derive(Debug)]
pub(super) struct SessionIdRow {
    pub(super) session_id: String,
}

impl<'r> sqlx::FromRow<'r, SqliteRow> for MetricSnapshotRow {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            timestamp: row.try_get("timestamp")?,
            active_sessions: row.try_get("active_sessions")?,
            total_sessions: row.try_get("total_sessions")?,
            bandwidth: row.try_get("bandwidth")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, SqliteRow> for SmtpConfigRow {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            enabled: row.try_get("enabled")?,
            mode: row.try_get("mode")?,
            host: row.try_get("host")?,
            port: row.try_get("port")?,
            from_address: row.try_get("from_address")?,
            from_name: row.try_get("from_name")?,
            username: row.try_get("username")?,
            password_encrypted: row.try_get("password_encrypted")?,
            notify_recipients: row.try_get("notify_recipients")?,
            notify_critical: row.try_get("notify_critical")?,
            notify_security: row.try_get("notify_security")?,
            notify_config_changes: row.try_get("notify_config_changes")?,
            notify_service_status: row.try_get("notify_service_status")?,
            notify_resource_pressure: row.try_get("notify_resource_pressure")?,
            notify_connection_pressure: row.try_get("notify_connection_pressure")?,
            notify_cooldown_seconds: row.try_get("notify_cooldown_seconds")?,
            notify_cpu_threshold: row.try_get("notify_cpu_threshold")?,
            notify_ram_threshold: row.try_get("notify_ram_threshold")?,
            notify_disk_threshold: row.try_get("notify_disk_threshold")?,
            notify_connection_percent_threshold: row
                .try_get("notify_connection_percent_threshold")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, SqliteRow> for SessionRow {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            session_id: row.try_get("session_id")?,
            user: row.try_get("user")?,
            start_time: row.try_get("start_time")?,
            end_time: row.try_get("end_time")?,
            duration_secs: row.try_get("duration_secs")?,
            source_ip: row.try_get("source_ip")?,
            source_port: row.try_get("source_port")?,
            dest_ip: row.try_get("dest_ip")?,
            dest_port: row.try_get("dest_port")?,
            protocol: row.try_get("protocol")?,
            bytes_sent: row.try_get("bytes_sent")?,
            bytes_received: row.try_get("bytes_received")?,
            packets_sent: row.try_get("packets_sent")?,
            packets_received: row.try_get("packets_received")?,
            status: row.try_get("status")?,
            close_reason: row.try_get("close_reason")?,
            acl_rule_matched: row.try_get("acl_rule_matched")?,
            acl_decision: row.try_get("acl_decision")?,
        })
    }
}

impl<'r> sqlx::FromRow<'r, SqliteRow> for SessionIdRow {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            session_id: row.try_get("session_id")?,
        })
    }
}

impl SessionRow {
    pub(super) fn into_session(self) -> Result<Session, sqlx::Error> {
        let session_id =
            Uuid::parse_str(&self.session_id).map_err(|e| decode_error("session_id", e))?;
        let start_time = parse_datetime("start_time", &self.start_time)?;
        let end_time = match self.end_time {
            Some(ref ts) => Some(parse_datetime("end_time", ts)?),
            None => None,
        };

        let protocol = self
            .protocol
            .parse::<SessionProtocol>()
            .map_err(|e| decode_error("protocol", e))?;

        let status = self
            .status
            .parse::<SessionStatus>()
            .map_err(|e| decode_error("status", e))?;

        let source_ip = self
            .source_ip
            .parse()
            .map_err(|e| decode_error("source_ip", e))?;

        Ok(Session {
            session_id,
            user: self.user.into(),
            start_time,
            end_time,
            duration_secs: sanitize_duration(self.duration_secs),
            connect_latency_ms: None,
            source_ip,
            source_port: u16::try_from(self.source_port).map_err(|_| {
                decode_error("source_port", format!("out of range: {}", self.source_port))
            })?,
            dest_ip: self.dest_ip.into(),
            dest_port: u16::try_from(self.dest_port).map_err(|_| {
                decode_error("dest_port", format!("out of range: {}", self.dest_port))
            })?,
            protocol,
            bytes_sent: self.bytes_sent as u64,
            bytes_received: self.bytes_received as u64,
            packets_sent: self.packets_sent as u64,
            packets_received: self.packets_received as u64,
            status,
            close_reason: self.close_reason,
            acl_rule_matched: self.acl_rule_matched.map(Arc::from),
            acl_decision: self.acl_decision.into(),
        })
    }
}

pub(super) struct SessionParams<'a> {
    pub(super) session_id: Cow<'a, str>,
    pub(super) user: Cow<'a, str>,
    pub(super) start_time: String,
    pub(super) end_time: Option<String>,
    pub(super) duration_secs: Option<i64>,
    pub(super) source_ip: Cow<'a, str>,
    pub(super) source_port: i64,
    pub(super) dest_ip: Cow<'a, str>,
    pub(super) dest_port: i64,
    pub(super) protocol: Cow<'a, str>,
    pub(super) bytes_sent: i64,
    pub(super) bytes_received: i64,
    pub(super) packets_sent: i64,
    pub(super) packets_received: i64,
    pub(super) status: Cow<'a, str>,
    pub(super) close_reason: Option<String>,
    pub(super) acl_rule_matched: Option<String>,
    pub(super) acl_decision: Cow<'a, str>,
}

impl<'a> From<&'a Session> for SessionParams<'a> {
    fn from(session: &'a Session) -> Self {
        Self {
            session_id: Cow::Owned(session.session_id.to_string()),
            user: Cow::Borrowed(session.user.as_ref()),
            start_time: session.start_time.to_rfc3339(),
            end_time: session.end_time.map(|dt: DateTime<Utc>| dt.to_rfc3339()),
            duration_secs: session.duration_secs.map(|v| v as i64),
            source_ip: Cow::Owned(session.source_ip.to_string()),
            source_port: session.source_port as i64,
            dest_ip: Cow::Borrowed(session.dest_ip.as_ref()),
            dest_port: session.dest_port as i64,
            protocol: Cow::Owned(session.protocol.to_string()),
            bytes_sent: session.bytes_sent as i64,
            bytes_received: session.bytes_received as i64,
            packets_sent: session.packets_sent as i64,
            packets_received: session.packets_received as i64,
            status: Cow::Borrowed(session.status.as_str()),
            close_reason: session.close_reason.clone(),
            acl_rule_matched: session
                .acl_rule_matched
                .as_ref()
                .map(|s: &Arc<str>| s.to_string()),
            acl_decision: Cow::Borrowed(session.acl_decision.as_ref()),
        }
    }
}

pub(super) fn parse_datetime(field: &str, value: &str) -> Result<DateTime<Utc>, sqlx::Error> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| parse_legacy_timestamp(value))
        .map_err(|e| decode_error(field, e))
}

fn parse_legacy_timestamp(value: &str) -> Result<DateTime<Utc>, chrono::format::ParseError> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
        .map(|naive| naive.and_utc())
}

pub(super) fn sanitize_duration(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| if v >= 0 { Some(v as u64) } else { None })
}

pub(super) fn decode_error(
    field: &str,
    err: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: field.into(),
        source: err.into(),
    }
}
