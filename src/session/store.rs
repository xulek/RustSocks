use super::types::{Protocol as SessionProtocol, Session, SessionFilter, SessionStatus};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Persistent storage for session history backed by SQLite.
#[derive(Debug)]
pub struct SessionStore {
    pool: SqlitePool,
}

impl SessionStore {
    /// Create a new store and apply migrations.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(database_url)?;

        // Ensure parent directory exists for file-based databases
        let options_clone = options.clone();
        let filename = options_clone.get_filename();
        let filename_path: &Path = filename;
        if filename_path != Path::new(":memory:") {
            if let Some(parent) = filename_path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
                }
            }

            if !filename_path.exists() {
                std::fs::File::create(filename_path).map_err(sqlx::Error::Io)?;
            }
        }

        let options = options.create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // Apply migrations shipped in the `migrations/` directory.
        if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
            return Err(sqlx::Error::Migrate(Box::new(e)));
        }

        Ok(Self { pool })
    }

    /// Access underlying connection pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Insert or update a session record.
    pub async fn insert_session(&self, session: &Session) -> Result<(), sqlx::Error> {
        self.upsert_session(session).await
    }

    /// Update an existing session record.
    pub async fn update_session(&self, session: &Session) -> Result<(), sqlx::Error> {
        self.upsert_session(session).await
    }

    /// Fetch sessions using provided filter.
    pub async fn query_sessions(
        &self,
        filter: &SessionFilter,
    ) -> Result<Vec<Session>, sqlx::Error> {
        let mut builder = QueryBuilder::new(
            r#"
            SELECT
                session_id,
                user,
                start_time,
                end_time,
                duration_secs,
                source_ip,
                source_port,
                dest_ip,
                dest_port,
                protocol,
                bytes_sent,
                bytes_received,
                packets_sent,
                packets_received,
                status,
                close_reason,
                acl_rule_matched,
                acl_decision
            FROM sessions
            WHERE 1=1
            "#,
        );

        Self::push_filters(&mut builder, filter);
        builder.push(" ORDER BY datetime(start_time) DESC ");

        if let Some(limit) = filter.limit {
            builder.push(" LIMIT ").push_bind(limit as i64);
        }

        if let Some(offset) = filter.offset {
            builder.push(" OFFSET ").push_bind(offset as i64);
        }

        let query = builder.build_query_as::<SessionRow>();
        let rows = query.fetch_all(&self.pool).await?;

        rows.into_iter().map(SessionRow::into_session).collect()
    }

    pub async fn count_sessions(&self, filter: &SessionFilter) -> Result<u64, sqlx::Error> {
        let mut builder = QueryBuilder::new(
            r#"
            SELECT COUNT(*) as count
            FROM sessions
            WHERE 1=1
            "#,
        );

        Self::push_filters(&mut builder, filter);
        let query = builder.build_query_scalar();
        let count: i64 = query.fetch_one(&self.pool).await?;
        Ok(count as u64)
    }

    pub async fn existing_session_ids(&self, ids: &[Uuid]) -> Result<HashSet<Uuid>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut builder = QueryBuilder::new(
            r#"
            SELECT session_id FROM sessions WHERE session_id IN (
            "#,
        );

        {
            let mut separated = builder.separated(", ");
            for id in ids {
                separated.push_bind(id.to_string());
            }
        }

        builder.push(")");

        let rows = builder
            .build_query_as::<SessionIdRow>()
            .fetch_all(&self.pool)
            .await?;

        let mut set = HashSet::with_capacity(rows.len());
        for row in rows {
            if let Ok(id) = Uuid::parse_str(&row.session_id) {
                set.insert(id);
            }
        }

        Ok(set)
    }

    pub async fn get_session(&self, session_id: &Uuid) -> Result<Option<Session>, sqlx::Error> {
        let mut builder = QueryBuilder::new(
            r#"
            SELECT
                session_id,
                user,
                start_time,
                end_time,
                duration_secs,
                source_ip,
                source_port,
                dest_ip,
                dest_port,
                protocol,
                bytes_sent,
                bytes_received,
                packets_sent,
                packets_received,
                status,
                close_reason,
                acl_rule_matched,
                acl_decision
            FROM sessions
            WHERE session_id = 
            "#,
        );
        builder.push_bind(session_id.to_string());
        let query = builder.build_query_as::<SessionRow>();

        let row = query.fetch_optional(&self.pool).await?;
        row.map(SessionRow::into_session).transpose()
    }

    fn push_filters(builder: &mut QueryBuilder<'_, Sqlite>, filter: &SessionFilter) {
        if let Some(user) = &filter.user {
            builder.push(" AND user = ").push_bind(user.clone());
        }

        if let Some(status) = &filter.status {
            builder
                .push(" AND status = ")
                .push_bind(status.as_str().to_string());
        }

        if let Some(start_after) = filter.start_after {
            builder
                .push(" AND datetime(start_time) >= datetime(")
                .push_bind(start_after.to_rfc3339())
                .push(")");
        }

        if let Some(start_before) = filter.start_before {
            builder
                .push(" AND datetime(start_time) <= datetime(")
                .push_bind(start_before.to_rfc3339())
                .push(")");
        }

        if let Some(dest_ip) = &filter.dest_ip {
            builder.push(" AND dest_ip = ").push_bind(dest_ip.clone());
        }

        if let Some(min_duration) = filter.min_duration_secs {
            builder
                .push(" AND duration_secs IS NOT NULL AND duration_secs >= ")
                .push_bind(min_duration as i64);
        }

        if let Some(min_bytes) = filter.min_bytes {
            builder.push(" AND (bytes_sent + bytes_received) >= ");
            builder.push_bind(min_bytes as i64);
        }
    }

    async fn upsert_session(&self, session: &Session) -> Result<(), sqlx::Error> {
        let params = SessionParams::from(session);

        sqlx::query(
            r#"
            INSERT INTO sessions (
                session_id,
                user,
                start_time,
                end_time,
                duration_secs,
                source_ip,
                source_port,
                dest_ip,
                dest_port,
                protocol,
                bytes_sent,
                bytes_received,
                packets_sent,
                packets_received,
                status,
                close_reason,
                acl_rule_matched,
                acl_decision
            )
            VALUES (
                ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
            )
            ON CONFLICT(session_id) DO UPDATE SET
                user = excluded.user,
                start_time = excluded.start_time,
                end_time = excluded.end_time,
                duration_secs = excluded.duration_secs,
                source_ip = excluded.source_ip,
                source_port = excluded.source_port,
                dest_ip = excluded.dest_ip,
                dest_port = excluded.dest_port,
                protocol = excluded.protocol,
                bytes_sent = excluded.bytes_sent,
                bytes_received = excluded.bytes_received,
                packets_sent = excluded.packets_sent,
                packets_received = excluded.packets_received,
                status = excluded.status,
                close_reason = excluded.close_reason,
                acl_rule_matched = excluded.acl_rule_matched,
                acl_decision = excluded.acl_decision
            "#,
        )
        .bind(&params.session_id)
        .bind(&params.user)
        .bind(&params.start_time)
        .bind(&params.end_time)
        .bind(params.duration_secs)
        .bind(&params.source_ip)
        .bind(params.source_port)
        .bind(&params.dest_ip)
        .bind(params.dest_port)
        .bind(&params.protocol)
        .bind(params.bytes_sent)
        .bind(params.bytes_received)
        .bind(params.packets_sent)
        .bind(params.packets_received)
        .bind(&params.status)
        .bind(&params.close_reason)
        .bind(&params.acl_rule_matched)
        .bind(&params.acl_decision)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn save_batch(&self, sessions: Vec<Session>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for session in sessions {
            let params = SessionParams::from(&session);
            sqlx::query(
                r#"
                INSERT INTO sessions (
                    session_id,
                    user,
                    start_time,
                    end_time,
                    duration_secs,
                    source_ip,
                    source_port,
                    dest_ip,
                    dest_port,
                    protocol,
                    bytes_sent,
                    bytes_received,
                    packets_sent,
                    packets_received,
                    status,
                    close_reason,
                    acl_rule_matched,
                    acl_decision
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(session_id) DO UPDATE SET
                    user = excluded.user,
                    start_time = excluded.start_time,
                    end_time = excluded.end_time,
                    duration_secs = excluded.duration_secs,
                    source_ip = excluded.source_ip,
                    source_port = excluded.source_port,
                    dest_ip = excluded.dest_ip,
                    dest_port = excluded.dest_port,
                    protocol = excluded.protocol,
                    bytes_sent = excluded.bytes_sent,
                    bytes_received = excluded.bytes_received,
                    packets_sent = excluded.packets_sent,
                    packets_received = excluded.packets_received,
                    status = excluded.status,
                    close_reason = excluded.close_reason,
                    acl_rule_matched = excluded.acl_rule_matched,
                    acl_decision = excluded.acl_decision
                "#,
            )
            .bind(&params.session_id)
            .bind(&params.user)
            .bind(&params.start_time)
            .bind(&params.end_time)
            .bind(params.duration_secs)
            .bind(&params.source_ip)
            .bind(params.source_port)
            .bind(&params.dest_ip)
            .bind(params.dest_port)
            .bind(&params.protocol)
            .bind(params.bytes_sent)
            .bind(params.bytes_received)
            .bind(params.packets_sent)
            .bind(params.packets_received)
            .bind(&params.status)
            .bind(&params.close_reason)
            .bind(&params.acl_rule_matched)
            .bind(&params.acl_decision)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    pub async fn cleanup_older_than(&self, retention_days: u64) -> Result<u64, sqlx::Error> {
        if retention_days == 0 {
            return Ok(0);
        }

        let cutoff = Utc::now() - ChronoDuration::days(retention_days as i64);

        let affected = sqlx::query(
            r#"
            DELETE FROM sessions
            WHERE datetime(start_time) < datetime(?);
            "#,
        )
        .bind(cutoff.to_rfc3339())
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(affected)
    }

    pub fn spawn_cleanup(self: &Arc<Self>, retention_days: u64, interval_hours: u64) {
        if retention_days == 0 {
            info!("Session cleanup disabled (retention_days = 0)");
            return;
        }

        let interval_secs = interval_hours.max(1) * 3600;
        let store = Arc::clone(self);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                match store.cleanup_older_than(retention_days).await {
                    Ok(affected) => {
                        if affected > 0 {
                            debug!(affected, "Session cleanup removed old records");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Session cleanup task failed");
                    }
                }
            }
        });

        info!(
            retention_days,
            interval_hours, "Session cleanup task started"
        );
    }

    /// Insert a metrics snapshot.
    pub async fn insert_metric(
        &self,
        timestamp: &DateTime<Utc>,
        active_sessions: u64,
        total_sessions: u64,
        bandwidth: u64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO metrics_snapshots (timestamp, active_sessions, total_sessions, bandwidth)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(timestamp.to_rfc3339())
        .bind(active_sessions as i64)
        .bind(total_sessions as i64)
        .bind(bandwidth as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Query metrics snapshots within a time range.
    pub async fn query_metrics(
        &self,
        start: Option<&DateTime<Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<MetricsSnapshot>, sqlx::Error> {
        let mut query = String::from(
            r#"
            SELECT timestamp, active_sessions, total_sessions, bandwidth
            FROM metrics_snapshots
            WHERE 1=1
            "#,
        );

        if start.is_some() {
            query.push_str(" AND datetime(timestamp) >= datetime(?)");
        }

        query.push_str(" ORDER BY timestamp DESC");

        if let Some(limit_val) = limit {
            query.push_str(&format!(" LIMIT {}", limit_val));
        }

        let mut q = sqlx::query_as::<_, MetricSnapshotRow>(&query);

        if let Some(start_time) = start {
            q = q.bind(start_time.to_rfc3339());
        }

        let rows = q.fetch_all(&self.pool).await?;

        rows.into_iter()
            .map(|row| row.into_metric())
            .collect::<Result<Vec<_>, _>>()
    }

    /// Cleanup old metrics snapshots.
    pub async fn cleanup_old_metrics(&self, retention_hours: u64) -> Result<u64, sqlx::Error> {
        if retention_hours == 0 {
            return Ok(0);
        }

        let cutoff = Utc::now() - ChronoDuration::hours(retention_hours as i64);

        let affected = sqlx::query(
            r#"
            DELETE FROM metrics_snapshots
            WHERE datetime(timestamp) < datetime(?);
            "#,
        )
        .bind(cutoff.to_rfc3339())
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(affected)
    }

    /// Spawn background task to cleanup old metrics.
    pub fn spawn_metrics_cleanup(self: &Arc<Self>, retention_hours: u64, interval_hours: u64) {
        if retention_hours == 0 {
            info!("Metrics cleanup disabled (retention_hours = 0)");
            return;
        }

        let interval_secs = interval_hours.max(1) * 3600;
        let store = Arc::clone(self);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                match store.cleanup_old_metrics(retention_hours).await {
                    Ok(affected) => {
                        if affected > 0 {
                            debug!(affected, "Metrics cleanup removed old records");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Metrics cleanup task failed");
                    }
                }
            }
        });

        info!(
            retention_hours,
            interval_hours, "Metrics cleanup task started"
        );
    }
}

#[derive(Debug, FromRow)]
struct MetricSnapshotRow {
    timestamp: String,
    active_sessions: i64,
    total_sessions: i64,
    bandwidth: i64,
}

impl MetricSnapshotRow {
    fn into_metric(self) -> Result<MetricsSnapshot, sqlx::Error> {
        let timestamp = parse_datetime("timestamp", &self.timestamp)?;

        Ok(MetricsSnapshot {
            timestamp,
            active_sessions: self.active_sessions as u64,
            total_sessions: self.total_sessions as u64,
            bandwidth: self.bandwidth as u64,
        })
    }
}

use super::history::MetricsSnapshot;

#[derive(Debug, FromRow)]
struct SessionRow {
    session_id: String,
    user: String,
    start_time: String,
    end_time: Option<String>,
    duration_secs: Option<i64>,
    source_ip: String,
    source_port: i64,
    dest_ip: String,
    dest_port: i64,
    protocol: String,
    bytes_sent: i64,
    bytes_received: i64,
    packets_sent: i64,
    packets_received: i64,
    status: String,
    close_reason: Option<String>,
    acl_rule_matched: Option<String>,
    acl_decision: String,
}

#[derive(Debug, FromRow)]
struct SessionIdRow {
    session_id: String,
}

impl SessionRow {
    fn into_session(self) -> Result<Session, sqlx::Error> {
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
            user: self.user,
            start_time,
            end_time,
            duration_secs: self.duration_secs.map(|v| v as u64),
            source_ip,
            source_port: self.source_port as u16,
            dest_ip: self.dest_ip,
            dest_port: self.dest_port as u16,
            protocol,
            bytes_sent: self.bytes_sent as u64,
            bytes_received: self.bytes_received as u64,
            packets_sent: self.packets_sent as u64,
            packets_received: self.packets_received as u64,
            status,
            close_reason: self.close_reason,
            acl_rule_matched: self.acl_rule_matched,
            acl_decision: self.acl_decision,
        })
    }
}

struct SessionParams<'a> {
    session_id: Cow<'a, str>,
    user: Cow<'a, str>,
    start_time: String,
    end_time: Option<String>,
    duration_secs: Option<i64>,
    source_ip: Cow<'a, str>,
    source_port: i64,
    dest_ip: Cow<'a, str>,
    dest_port: i64,
    protocol: Cow<'a, str>,
    bytes_sent: i64,
    bytes_received: i64,
    packets_sent: i64,
    packets_received: i64,
    status: Cow<'a, str>,
    close_reason: Option<String>,
    acl_rule_matched: Option<String>,
    acl_decision: Cow<'a, str>,
}

impl<'a> From<&'a Session> for SessionParams<'a> {
    fn from(session: &'a Session) -> Self {
        Self {
            session_id: Cow::Owned(session.session_id.to_string()),
            user: Cow::Borrowed(session.user.as_str()),
            start_time: session.start_time.to_rfc3339(),
            end_time: session.end_time.map(|dt| dt.to_rfc3339()),
            duration_secs: session.duration_secs.map(|v| v as i64),
            source_ip: Cow::Owned(session.source_ip.to_string()),
            source_port: session.source_port as i64,
            dest_ip: Cow::Borrowed(session.dest_ip.as_str()),
            dest_port: session.dest_port as i64,
            protocol: Cow::Owned(session.protocol.to_string()),
            bytes_sent: session.bytes_sent as i64,
            bytes_received: session.bytes_received as i64,
            packets_sent: session.packets_sent as i64,
            packets_received: session.packets_received as i64,
            status: Cow::Borrowed(session.status.as_str()),
            close_reason: session.close_reason.clone(),
            acl_rule_matched: session.acl_rule_matched.clone(),
            acl_decision: Cow::Borrowed(session.acl_decision.as_str()),
        }
    }
}

fn parse_datetime(field: &str, value: &str) -> Result<DateTime<Utc>, sqlx::Error> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| decode_error(field, e))
}

fn decode_error(
    field: &str,
    err: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: field.into(),
        source: err.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ConnectionInfo, SessionProtocol};
    use std::net::{IpAddr, Ipv4Addr};

    fn test_session() -> Session {
        let conn = ConnectionInfo {
            source_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            source_port: 5000,
            dest_ip: "example.com".to_string(),
            dest_port: 443,
            protocol: SessionProtocol::Tcp,
        };

        let mut session = Session::new("alice", conn, "allow", Some("Allow HTTPS".into()));
        session.bytes_sent = 2048;
        session.bytes_received = 1024;
        session.packets_sent = 15;
        session.packets_received = 12;
        session
    }

    #[tokio::test]
    async fn store_and_query_session() {
        let store = SessionStore::connect("sqlite::memory:").await.unwrap();

        let mut session = test_session();
        store.insert_session(&session).await.unwrap();

        let mut results = store
            .query_sessions(&SessionFilter::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].user, "alice");

        // Close session and persist update
        session.close(Some("Finished".into()), SessionStatus::Closed);
        store.update_session(&session).await.unwrap();

        // Batch save should upsert without error
        store
            .save_batch(vec![session.clone()])
            .await
            .expect("batch upsert");

        let filter = SessionFilter {
            status: Some(SessionStatus::Closed),
            ..Default::default()
        };
        results = store.query_sessions(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SessionStatus::Closed);
        assert!(results[0].end_time.is_some());
    }
}
