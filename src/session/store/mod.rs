mod models;
mod mysql;
mod sqlite;

use super::types::{Session, SessionFilter};
use crate::session::history::MetricsSnapshot;
use crate::smtp::encryption::encrypt_password;
use crate::smtp::types::{format_recipients, SmtpConfig};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use mysql_async::prelude::Queryable;
use mysql_async::{Pool as MySqlPool, Row as MySqlRow, TxOpts};
use sqlx_sqlite::{Sqlite, SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, info, warn};
use uuid::Uuid;

use self::models::{
    parse_datetime, MetricSnapshotRow, SessionIdRow, SessionParams, SessionRow, SmtpConfigRow,
};
use self::mysql::{
    mysql_error_to_sqlx, mysql_index_statements, mysql_params, mysql_row_to_session_row,
    mysql_row_to_smtp_config_row, mysql_schema_statements, mysql_session_params,
    mysql_smtp_columns, mysql_sort_column, mysql_upsert_session_sql, push_mysql_filters,
    sort_direction, MySqlMetricTuple, MySqlParam,
};

mod sqlx {
    pub use sqlx_core::error::Error;
    pub use sqlx_core::from_row::FromRow;
    pub use sqlx_core::query::query;
    pub use sqlx_core::query_as::query_as;
    pub use sqlx_core::query_builder::QueryBuilder;
    pub use sqlx_core::query_scalar::query_scalar;

    pub mod migrate {
        pub use sqlx_core::migrate::{MigrateError, Migrator};
    }
}

use sqlx::QueryBuilder;

/// Persistent storage for session history.
#[derive(Debug)]
pub struct SessionStore {
    backend: StoreBackend,
    flavor: DatabaseFlavor,
}

#[derive(Debug)]
enum StoreBackend {
    Sqlite(SqlitePool),
    MySql(MySqlBackend),
}

#[derive(Debug)]
struct MySqlBackend {
    pool: MySqlPool,
}

#[derive(Debug, Clone)]
enum DatabaseFlavor {
    Sqlite {
        db_path: Option<PathBuf>,
        is_memory: bool,
        connect_url: String,
    },
    MariaDb,
}

impl SessionStore {
    /// Create a new store and apply migrations.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let mut allow_reset = true;

        loop {
            match Self::connect_attempt(database_url, allow_reset).await? {
                Some(store) => return Ok(store),
                None => {
                    allow_reset = false;
                    continue;
                }
            }
        }
    }

    async fn connect_attempt(
        database_url: &str,
        allow_reset: bool,
    ) -> Result<Option<Self>, sqlx::Error> {
        let flavor = DatabaseFlavor::from_url(database_url)?;

        if let Some(path) = flavor.sqlite_path() {
            sqlite::preflight_database_file(path)?;

            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
                }
            }

            if !path.exists() {
                std::fs::File::create(path).map_err(sqlx::Error::Io)?;
            }

            sqlite::create_database_backup(path)?;
        }

        let backend = match &flavor {
            DatabaseFlavor::Sqlite { connect_url, .. } => {
                let pool = SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect(connect_url)
                    .await?;

                sqlite::apply_migrations(&pool, &flavor).await?;
                StoreBackend::Sqlite(pool)
            }
            DatabaseFlavor::MariaDb => {
                let mysql = MySqlBackend::connect(flavor.connection_url(database_url)).await?;
                StoreBackend::MySql(mysql)
            }
        };

        if flavor.is_sqlite() {
            let pool = match &backend {
                StoreBackend::Sqlite(pool) => pool,
                StoreBackend::MySql(_) => unreachable!("sqlite flavor must use sqlite backend"),
            };
            sqlite::configure_journal_mode(pool).await?;

            // Optimize for performance with 500k+ rows (SQLite-only)
            sqlx::query("PRAGMA synchronous = FULL")
                .execute(pool)
                .await?;

            sqlx::query("PRAGMA cache_size = -64000")
                .execute(pool)
                .await?;

            sqlx::query("PRAGMA page_size = 8192").execute(pool).await?;

            sqlx::query("PRAGMA mmap_size = 268435456")
                .execute(pool)
                .await?;

            sqlx::query("PRAGMA optimize").execute(pool).await?;

            info!(
                "SQLite optimizations enabled: safe journaling, 64MB cache, 8KB pages, 256MB mmap"
            );

            if let Err(e) = sqlite::verify_integrity(pool).await {
                warn!(
                    error = %e,
                    "SQLite integrity check failed"
                );
                if allow_reset {
                    pool.close().await;
                    if flavor.is_memory() {
                        warn!("Reinitializing in-memory database after failed integrity check");
                    } else if let Some(path) = flavor.sqlite_path() {
                        sqlite::quarantine_database_file(path)?;
                        warn!("Corrupted database quarantined; recreating a fresh database");
                    }
                    return Ok(None);
                } else {
                    return Err(e);
                }
            }
        }

        Ok(Some(Self { backend, flavor }))
    }

    fn is_in_memory_database(filename: &Path, url: &str) -> bool {
        if filename == Path::new(":memory:") {
            return true;
        }

        let filename_str = filename.to_string_lossy();
        if filename_str.starts_with("file:sqlx-in-memory") {
            return true;
        }

        let url_lower = url.to_ascii_lowercase();
        url_lower.contains(":memory:") || url_lower.contains("mode=memory")
    }

    #[cfg(test)]
    pub(crate) async fn close_for_test(&self) {
        match &self.backend {
            StoreBackend::Sqlite(pool) => pool.close().await,
            StoreBackend::MySql(mysql) => {
                let _ = mysql
                    .pool
                    .clone()
                    .disconnect()
                    .await
                    .map_err(mysql_error_to_sqlx);
            }
        }
    }

    fn sqlite_pool(&self) -> &SqlitePool {
        match &self.backend {
            StoreBackend::Sqlite(pool) => pool,
            StoreBackend::MySql(_) => unreachable!("sqlite pool requested for mysql backend"),
        }
    }

    /// Mark all active sessions as closed (called on server startup to clean up stale sessions).
    pub async fn close_all_active_sessions(&self) -> Result<u64, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.close_all_active_sessions().await;
        }

        let now = Utc::now();
        let result = sqlx::query(
            r#"
            UPDATE sessions
            SET status = 'closed',
                close_reason = 'Server restart',
                end_time = ?,
                duration_secs = CAST((julianday(?) - julianday(start_time)) * 86400 AS INTEGER)
            WHERE status = 'active'
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(self.sqlite_pool())
        .await?;

        let rows_affected = result.rows_affected();
        if rows_affected > 0 {
            info!(
                count = rows_affected,
                "Marked active sessions as closed on startup"
            );
        }
        Ok(rows_affected)
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
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.query_sessions(filter).await;
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
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

        // Apply sorting (with validation to prevent SQL injection)
        let sort_column = filter.sort_by.as_deref().unwrap_or("start_time");
        let sort_column = match sort_column {
            "user" => "user",
            "source_ip" => "source_ip",
            "dest_ip" => "dest_ip",
            "protocol" => "protocol",
            "status" => "status",
            "acl_decision" => "acl_decision",
            "bytes_sent" => "bytes_sent",
            "bytes_received" => "bytes_received",
            "duration_seconds" | "duration_secs" => "duration_secs",
            "start_time" => "start_time",
            other => {
                tracing::warn!(column = %other, "Invalid sort column requested, falling back to start_time");
                "start_time"
            }
        };

        let sort_dir = filter.sort_dir.as_deref().unwrap_or("desc");
        let sort_dir = if sort_dir.eq_ignore_ascii_case("asc") {
            "ASC"
        } else {
            "DESC"
        };

        builder.push(format!(" ORDER BY {} {} ", sort_column, sort_dir));

        if let Some(limit) = filter.limit {
            builder.push(" LIMIT ").push_bind(limit as i64);
        }

        if let Some(offset) = filter.offset {
            builder.push(" OFFSET ").push_bind(offset as i64);
        }

        let query = builder.build_query_as::<SessionRow>();
        let rows = query.fetch_all(self.sqlite_pool()).await?;

        rows.into_iter().map(SessionRow::into_session).collect()
    }

    pub async fn count_sessions(&self, filter: &SessionFilter) -> Result<u64, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.count_sessions(filter).await;
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
            r#"
            SELECT COUNT(*) as count
            FROM sessions
            WHERE 1=1
            "#,
        );

        Self::push_filters(&mut builder, filter);
        let query = builder.build_query_scalar();
        let count: i64 = query.fetch_one(self.sqlite_pool()).await?;
        Ok(count as u64)
    }

    /// Get approximate total count of sessions (fast for large tables)
    ///
    /// Uses SQLite's internal statistics instead of scanning all rows.
    /// Much faster than COUNT(*) for tables with 100k+ rows.
    pub async fn approximate_total_sessions(&self) -> Result<u64, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.approximate_total_sessions().await;
        }

        if self.flavor.is_sqlite() {
            // Try to get approximate count from sqlite_stat1 (updated by ANALYZE)
            let result: Option<(i64,)> = sqlx::query_as(
                r#"
                SELECT stat FROM sqlite_stat1
                WHERE tbl = 'sessions' AND idx IS NULL
                "#,
            )
            .fetch_optional(self.sqlite_pool())
            .await?;

            if let Some((stat,)) = result {
                // Parse the stat string (format: "N" where N is row count)
                if let Ok(count) = stat
                    .to_string()
                    .split_whitespace()
                    .next()
                    .unwrap_or("0")
                    .parse::<u64>()
                {
                    debug!(count = count, "Using approximate count from sqlite_stat1");
                    return Ok(count);
                }
            }

            // Fallback: use max rowid as approximation (very fast)
            let max_rowid: Option<i64> = sqlx::query_scalar("SELECT MAX(ROWID) FROM sessions")
                .fetch_optional(self.sqlite_pool())
                .await?;

            let approx = max_rowid.unwrap_or(0) as u64;
            debug!(approx = approx, "Using max ROWID as approximate count");
            return Ok(approx);
        }

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(self.sqlite_pool())
            .await?;
        Ok(count as u64)
    }

    pub async fn existing_session_ids(&self, ids: &[Uuid]) -> Result<HashSet<Uuid>, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.existing_session_ids(ids).await;
        }

        if ids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
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
            .fetch_all(self.sqlite_pool())
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
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.get_session(session_id).await;
        }

        let mut builder = QueryBuilder::<Sqlite>::new(
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

        let row = query.fetch_optional(self.sqlite_pool()).await?;
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
            // Direct comparison works with RFC3339 format (sortable strings)
            builder
                .push(" AND start_time >= ")
                .push_bind(start_after.to_rfc3339());
        }

        if let Some(start_before) = filter.start_before {
            // Direct comparison works with RFC3339 format (sortable strings)
            builder
                .push(" AND start_time <= ")
                .push_bind(start_before.to_rfc3339());
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
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.upsert_session(session).await;
        }

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
        .bind(params.session_id.as_ref())
        .bind(params.user.as_ref())
        .bind(&params.start_time)
        .bind(&params.end_time)
        .bind(params.duration_secs)
        .bind(params.source_ip.as_ref())
        .bind(params.source_port)
        .bind(params.dest_ip.as_ref())
        .bind(params.dest_port)
        .bind(params.protocol.as_ref())
        .bind(params.bytes_sent)
        .bind(params.bytes_received)
        .bind(params.packets_sent)
        .bind(params.packets_received)
        .bind(params.status.as_ref())
        .bind(&params.close_reason)
        .bind(&params.acl_rule_matched)
        .bind(params.acl_decision.as_ref())
        .execute(self.sqlite_pool())
        .await?;

        Ok(())
    }

    pub async fn save_batch(&self, sessions: Vec<Session>) -> Result<(), sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.save_batch(sessions).await;
        }

        if sessions.is_empty() {
            return Ok(());
        }

        const MAX_ROWS_PER_QUERY: usize = 500;
        let mut tx = self.sqlite_pool().begin().await?;

        for chunk in sessions.chunks(MAX_ROWS_PER_QUERY) {
            let mut builder = QueryBuilder::<Sqlite>::new(
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
                "#,
            );

            builder.push_values(chunk.iter(), |mut row, session| {
                let SessionParams {
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
                    acl_decision,
                } = SessionParams::from(session);

                row.push_bind(session_id.into_owned())
                    .push_bind(user.into_owned())
                    .push_bind(start_time)
                    .push_bind(end_time)
                    .push_bind(duration_secs)
                    .push_bind(source_ip.into_owned())
                    .push_bind(source_port)
                    .push_bind(dest_ip.into_owned())
                    .push_bind(dest_port)
                    .push_bind(protocol.into_owned())
                    .push_bind(bytes_sent)
                    .push_bind(bytes_received)
                    .push_bind(packets_sent)
                    .push_bind(packets_received)
                    .push_bind(status.into_owned())
                    .push_bind(close_reason)
                    .push_bind(acl_rule_matched)
                    .push_bind(acl_decision.into_owned());
            });

            builder.push(
                r#"
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
            );

            builder.build().execute(&mut *tx).await?;
        }

        tx.commit().await
    }

    pub async fn cleanup_older_than(&self, retention_days: u64) -> Result<u64, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.cleanup_older_than(retention_days).await;
        }

        if retention_days == 0 {
            return Ok(0);
        }

        let cutoff = Utc::now() - ChronoDuration::days(retention_days as i64);

        let affected = sqlx::query(
            r#"
            DELETE FROM sessions
            WHERE start_time < ?;
            "#,
        )
        .bind(cutoff.to_rfc3339())
        .execute(self.sqlite_pool())
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
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql
                .insert_metric(timestamp, active_sessions, total_sessions, bandwidth)
                .await;
        }

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
        .execute(self.sqlite_pool())
        .await?;

        Ok(())
    }

    /// Query metrics snapshots within a time range.
    pub async fn query_metrics(
        &self,
        start: Option<&DateTime<Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<MetricsSnapshot>, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.query_metrics(start, limit).await;
        }

        let mut query = String::from(
            r#"
            SELECT timestamp, active_sessions, total_sessions, bandwidth
            FROM metrics_snapshots
            WHERE 1=1
            "#,
        );

        if start.is_some() {
            query.push_str(" AND timestamp >= ?");
        }

        query.push_str(" ORDER BY timestamp DESC");

        if let Some(limit_val) = limit {
            query.push_str(&format!(" LIMIT {}", limit_val));
        }

        let mut q = sqlx::query_as::<_, MetricSnapshotRow>(&query);

        if let Some(start_time) = start {
            q = q.bind(start_time.to_rfc3339());
        }

        let rows = q.fetch_all(self.sqlite_pool()).await?;

        rows.into_iter()
            .map(|row| row.into_metric())
            .collect::<Result<Vec<_>, _>>()
    }

    /// Cleanup old metrics snapshots.
    pub async fn cleanup_old_metrics(&self, retention_hours: u64) -> Result<u64, sqlx::Error> {
        if let StoreBackend::MySql(mysql) = &self.backend {
            return mysql.cleanup_old_metrics(retention_hours).await;
        }

        if retention_hours == 0 {
            return Ok(0);
        }

        let cutoff = Utc::now() - ChronoDuration::hours(retention_hours as i64);

        let affected = sqlx::query(
            r#"
            DELETE FROM metrics_snapshots
            WHERE timestamp < ?;
            "#,
        )
        .bind(cutoff.to_rfc3339())
        .execute(self.sqlite_pool())
        .await?
        .rows_affected();

        Ok(affected)
    }

    pub async fn load_smtp_config(&self, api_token: Option<&str>) -> Result<SmtpConfig, String> {
        let row = match &self.backend {
            StoreBackend::Sqlite(pool) => sqlx::query_as::<_, SmtpConfigRow>(
                "SELECT enabled, mode, host, port, from_address, from_name, username, password_encrypted, notify_recipients, notify_critical, notify_security, notify_config_changes, notify_service_status, notify_resource_pressure, notify_connection_pressure, notify_cooldown_seconds, notify_cpu_threshold, notify_ram_threshold, notify_disk_threshold, notify_connection_percent_threshold FROM smtp_config WHERE id = 1",
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?,
            StoreBackend::MySql(mysql) => mysql.load_smtp_row().await.map_err(|e| e.to_string())?,
        };

        Ok(row
            .map(|value| value.into_smtp_config(api_token))
            .transpose()?
            .unwrap_or_default())
    }

    pub async fn save_smtp_config(
        &self,
        config: &SmtpConfig,
        new_password: Option<&str>,
        api_token: Option<&str>,
    ) -> Result<(), String> {
        let password_encrypted = match new_password {
            Some(password) if !password.is_empty() => {
                let token = api_token.ok_or("API token required for password encryption")?;
                Some(encrypt_password(password, token)?)
            }
            _ => None,
        };
        let notify_recipients = format_recipients(&config.notify_recipients);

        match &self.backend {
            StoreBackend::Sqlite(pool) => {
                let exists: (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM smtp_config WHERE id = 1")
                        .fetch_one(pool)
                        .await
                        .map_err(|e| format!("Database error: {}", e))?;

                if exists.0 > 0 {
                    if password_encrypted.is_some() {
                        sqlx::query(
                            "UPDATE smtp_config SET enabled = ?, mode = ?, host = ?, port = ?, from_address = ?, from_name = ?, username = ?, password_encrypted = ?, notify_recipients = ?, notify_critical = ?, notify_security = ?, notify_config_changes = ?, notify_service_status = ?, notify_resource_pressure = ?, notify_connection_pressure = ?, notify_cooldown_seconds = ?, notify_cpu_threshold = ?, notify_ram_threshold = ?, notify_disk_threshold = ?, notify_connection_percent_threshold = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
                        )
                        .bind(if config.enabled { 1 } else { 0 })
                        .bind(config.mode.to_string())
                        .bind(&config.host)
                        .bind(config.port as i32)
                        .bind(&config.from_address)
                        .bind(&config.from_name)
                        .bind(&config.username)
                        .bind(&password_encrypted)
                        .bind(&notify_recipients)
                        .bind(if config.notify_critical { 1 } else { 0 })
                        .bind(if config.notify_security { 1 } else { 0 })
                        .bind(if config.notify_config_changes { 1 } else { 0 })
                        .bind(if config.notify_service_status { 1 } else { 0 })
                        .bind(if config.notify_resource_pressure { 1 } else { 0 })
                        .bind(if config.notify_connection_pressure { 1 } else { 0 })
                        .bind(config.notify_cooldown_seconds as i64)
                        .bind(config.notify_cpu_threshold as i32)
                        .bind(config.notify_ram_threshold as i32)
                        .bind(config.notify_disk_threshold as i32)
                        .bind(config.notify_connection_percent_threshold as i32)
                        .execute(pool)
                        .await
                        .map_err(|e| format!("Failed to update config: {}", e))?;
                    } else {
                        sqlx::query(
                            "UPDATE smtp_config SET enabled = ?, mode = ?, host = ?, port = ?, from_address = ?, from_name = ?, username = ?, notify_recipients = ?, notify_critical = ?, notify_security = ?, notify_config_changes = ?, notify_service_status = ?, notify_resource_pressure = ?, notify_connection_pressure = ?, notify_cooldown_seconds = ?, notify_cpu_threshold = ?, notify_ram_threshold = ?, notify_disk_threshold = ?, notify_connection_percent_threshold = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
                        )
                        .bind(if config.enabled { 1 } else { 0 })
                        .bind(config.mode.to_string())
                        .bind(&config.host)
                        .bind(config.port as i32)
                        .bind(&config.from_address)
                        .bind(&config.from_name)
                        .bind(&config.username)
                        .bind(&notify_recipients)
                        .bind(if config.notify_critical { 1 } else { 0 })
                        .bind(if config.notify_security { 1 } else { 0 })
                        .bind(if config.notify_config_changes { 1 } else { 0 })
                        .bind(if config.notify_service_status { 1 } else { 0 })
                        .bind(if config.notify_resource_pressure { 1 } else { 0 })
                        .bind(if config.notify_connection_pressure { 1 } else { 0 })
                        .bind(config.notify_cooldown_seconds as i64)
                        .bind(config.notify_cpu_threshold as i32)
                        .bind(config.notify_ram_threshold as i32)
                        .bind(config.notify_disk_threshold as i32)
                        .bind(config.notify_connection_percent_threshold as i32)
                        .execute(pool)
                        .await
                        .map_err(|e| format!("Failed to update config: {}", e))?;
                    }
                } else {
                    sqlx::query(
                        "INSERT INTO smtp_config (id, enabled, mode, host, port, from_address, from_name, username, password_encrypted, notify_recipients, notify_critical, notify_security, notify_config_changes, notify_service_status, notify_resource_pressure, notify_connection_pressure, notify_cooldown_seconds, notify_cpu_threshold, notify_ram_threshold, notify_disk_threshold, notify_connection_percent_threshold) VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(if config.enabled { 1 } else { 0 })
                    .bind(config.mode.to_string())
                    .bind(&config.host)
                    .bind(config.port as i32)
                    .bind(&config.from_address)
                    .bind(&config.from_name)
                    .bind(&config.username)
                    .bind(&password_encrypted)
                    .bind(&notify_recipients)
                    .bind(if config.notify_critical { 1 } else { 0 })
                    .bind(if config.notify_security { 1 } else { 0 })
                    .bind(if config.notify_config_changes { 1 } else { 0 })
                    .bind(if config.notify_service_status { 1 } else { 0 })
                    .bind(if config.notify_resource_pressure { 1 } else { 0 })
                    .bind(if config.notify_connection_pressure { 1 } else { 0 })
                    .bind(config.notify_cooldown_seconds as i64)
                    .bind(config.notify_cpu_threshold as i32)
                    .bind(config.notify_ram_threshold as i32)
                    .bind(config.notify_disk_threshold as i32)
                    .bind(config.notify_connection_percent_threshold as i32)
                    .execute(pool)
                    .await
                    .map_err(|e| format!("Failed to insert config: {}", e))?;
                }
            }
            StoreBackend::MySql(mysql) => {
                mysql
                    .save_smtp_config(config, password_encrypted, notify_recipients)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }

        Ok(())
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

impl SessionStore {}

impl MySqlBackend {
    async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = MySqlPool::new(database_url);
        let mut conn = pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        Self::ensure_schema(&mut conn).await?;
        drop(conn);
        Ok(Self { pool })
    }

    async fn close_all_active_sessions(&self) -> Result<u64, sqlx::Error> {
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let active_rows: Vec<(String, String)> = conn
            .exec(
                "SELECT session_id, start_time FROM sessions WHERE status = 'active'",
                (),
            )
            .await
            .map_err(mysql_error_to_sqlx)?;

        if active_rows.is_empty() {
            return Ok(0);
        }

        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let mut tx = self
            .pool
            .start_transaction(TxOpts::default())
            .await
            .map_err(mysql_error_to_sqlx)?;

        for (session_id, start_time) in &active_rows {
            let started = parse_datetime("start_time", start_time)?;
            let duration = (now - started).num_seconds().max(0);
            tx.exec_drop(
                r#"
                UPDATE sessions
                SET status = ?, close_reason = ?, end_time = ?, duration_secs = ?
                WHERE session_id = ?
                "#,
                mysql_params(vec![
                    MySqlParam::Text("closed".to_string()),
                    MySqlParam::Text("Server restart".to_string()),
                    MySqlParam::Text(now_str.clone()),
                    MySqlParam::OptI64(Some(duration)),
                    MySqlParam::Text(session_id.clone()),
                ]),
            )
            .await
            .map_err(mysql_error_to_sqlx)?;
        }

        tx.commit().await.map_err(mysql_error_to_sqlx)?;
        Ok(active_rows.len() as u64)
    }

    async fn query_sessions(&self, filter: &SessionFilter) -> Result<Vec<Session>, sqlx::Error> {
        let mut sql = String::from(
            r#"
            SELECT
                session_id,
                `user`,
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
        let mut params = Vec::new();
        push_mysql_filters(&mut sql, &mut params, filter);
        sql.push_str(" ORDER BY ");
        sql.push_str(mysql_sort_column(filter.sort_by.as_deref()));
        sql.push(' ');
        sql.push_str(sort_direction(filter.sort_dir.as_deref()));

        if let Some(limit) = filter.limit {
            sql.push_str(" LIMIT ?");
            params.push(MySqlParam::I64(limit as i64));
        }

        if let Some(offset) = filter.offset {
            sql.push_str(" OFFSET ?");
            params.push(MySqlParam::I64(offset as i64));
        }

        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let rows: Vec<MySqlRow> = conn
            .exec(sql, mysql_params(params))
            .await
            .map_err(mysql_error_to_sqlx)?;

        rows.into_iter()
            .map(mysql_row_to_session_row)
            .map(SessionRow::into_session)
            .collect()
    }

    async fn count_sessions(&self, filter: &SessionFilter) -> Result<u64, sqlx::Error> {
        let mut sql = String::from("SELECT COUNT(*) FROM sessions WHERE 1=1");
        let mut params = Vec::new();
        push_mysql_filters(&mut sql, &mut params, filter);

        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let count: Option<(u64,)> = conn
            .exec_first(sql, mysql_params(params))
            .await
            .map_err(mysql_error_to_sqlx)?;
        Ok(count.map(|(value,)| value).unwrap_or(0))
    }

    async fn approximate_total_sessions(&self) -> Result<u64, sqlx::Error> {
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let count: Option<(u64,)> = conn
            .exec_first("SELECT COUNT(*) FROM sessions", ())
            .await
            .map_err(mysql_error_to_sqlx)?;
        Ok(count.map(|(value,)| value).unwrap_or(0))
    }

    async fn existing_session_ids(&self, ids: &[Uuid]) -> Result<HashSet<Uuid>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }

        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT session_id FROM sessions WHERE session_id IN ({})",
            placeholders
        );
        let params = ids
            .iter()
            .map(|id| MySqlParam::Text(id.to_string()))
            .collect::<Vec<_>>();

        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let rows: Vec<(String,)> = conn
            .exec(sql, mysql_params(params))
            .await
            .map_err(mysql_error_to_sqlx)?;

        let mut set = HashSet::with_capacity(rows.len());
        for (session_id,) in rows {
            if let Ok(id) = Uuid::parse_str(&session_id) {
                set.insert(id);
            }
        }
        Ok(set)
    }

    async fn get_session(&self, session_id: &Uuid) -> Result<Option<Session>, sqlx::Error> {
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let row: Option<MySqlRow> = conn
            .exec_first(
                r#"
                SELECT
                    session_id,
                    `user`,
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
                WHERE session_id = ?
                "#,
                (session_id.to_string(),),
            )
            .await
            .map_err(mysql_error_to_sqlx)?;

        row.map(mysql_row_to_session_row)
            .map(SessionRow::into_session)
            .transpose()
    }

    async fn upsert_session(&self, session: &Session) -> Result<(), sqlx::Error> {
        let params = SessionParams::from(session);
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        conn.exec_drop(mysql_upsert_session_sql(), mysql_session_params(params))
            .await
            .map_err(mysql_error_to_sqlx)?;
        Ok(())
    }

    async fn save_batch(&self, sessions: Vec<Session>) -> Result<(), sqlx::Error> {
        if sessions.is_empty() {
            return Ok(());
        }

        let param_batches = sessions
            .iter()
            .map(SessionParams::from)
            .map(mysql_session_params)
            .collect::<Vec<_>>();

        let mut tx = self
            .pool
            .start_transaction(TxOpts::default())
            .await
            .map_err(mysql_error_to_sqlx)?;
        tx.exec_batch(mysql_upsert_session_sql(), param_batches)
            .await
            .map_err(mysql_error_to_sqlx)?;
        tx.commit().await.map_err(mysql_error_to_sqlx)?;
        Ok(())
    }

    async fn cleanup_older_than(&self, retention_days: u64) -> Result<u64, sqlx::Error> {
        if retention_days == 0 {
            return Ok(0);
        }

        let cutoff = Utc::now() - ChronoDuration::days(retention_days as i64);
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        conn.exec_drop(
            "DELETE FROM sessions WHERE start_time < ?",
            (cutoff.to_rfc3339(),),
        )
        .await
        .map_err(mysql_error_to_sqlx)?;
        Ok(conn.affected_rows())
    }

    async fn load_smtp_row(&self) -> Result<Option<SmtpConfigRow>, sqlx::Error> {
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let row: Option<MySqlRow> = conn
            .exec_first(
                r#"
                SELECT
                    enabled,
                    mode,
                    host,
                    port,
                    from_address,
                    from_name,
                    username,
                    password_encrypted,
                    notify_recipients,
                    notify_critical,
                    notify_security,
                    notify_config_changes,
                    notify_service_status,
                    notify_resource_pressure,
                    notify_connection_pressure,
                    notify_cooldown_seconds,
                    notify_cpu_threshold,
                    notify_ram_threshold,
                    notify_disk_threshold,
                    notify_connection_percent_threshold
                FROM smtp_config
                WHERE id = 1
                "#,
                (),
            )
            .await
            .map_err(mysql_error_to_sqlx)?;
        Ok(row.map(mysql_row_to_smtp_config_row))
    }

    async fn save_smtp_config(
        &self,
        config: &SmtpConfig,
        password_encrypted: Option<String>,
        notify_recipients: String,
    ) -> Result<(), sqlx::Error> {
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        conn.exec_drop(
            r#"
            INSERT INTO smtp_config (
                id,
                enabled,
                mode,
                host,
                port,
                from_address,
                from_name,
                username,
                password_encrypted,
                notify_recipients,
                notify_critical,
                notify_security,
                notify_config_changes,
                notify_service_status,
                notify_resource_pressure,
                notify_connection_pressure,
                notify_cooldown_seconds,
                notify_cpu_threshold,
                notify_ram_threshold,
                notify_disk_threshold,
                notify_connection_percent_threshold
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON DUPLICATE KEY UPDATE
                enabled = VALUES(enabled),
                mode = VALUES(mode),
                host = VALUES(host),
                port = VALUES(port),
                from_address = VALUES(from_address),
                from_name = VALUES(from_name),
                username = VALUES(username),
                password_encrypted = COALESCE(VALUES(password_encrypted), password_encrypted),
                notify_recipients = VALUES(notify_recipients),
                notify_critical = VALUES(notify_critical),
                notify_security = VALUES(notify_security),
                notify_config_changes = VALUES(notify_config_changes),
                notify_service_status = VALUES(notify_service_status),
                notify_resource_pressure = VALUES(notify_resource_pressure),
                notify_connection_pressure = VALUES(notify_connection_pressure),
                notify_cooldown_seconds = VALUES(notify_cooldown_seconds),
                notify_cpu_threshold = VALUES(notify_cpu_threshold),
                notify_ram_threshold = VALUES(notify_ram_threshold),
                notify_disk_threshold = VALUES(notify_disk_threshold),
                notify_connection_percent_threshold = VALUES(notify_connection_percent_threshold),
                updated_at = CURRENT_TIMESTAMP
            "#,
            mysql_params(vec![
                MySqlParam::I64(1),
                MySqlParam::I64(if config.enabled { 1 } else { 0 }),
                MySqlParam::Text(config.mode.to_string()),
                MySqlParam::Text(config.host.clone()),
                MySqlParam::I64(config.port as i64),
                MySqlParam::Text(config.from_address.clone()),
                MySqlParam::OptText(config.from_name.clone()),
                MySqlParam::OptText(config.username.clone()),
                MySqlParam::OptText(password_encrypted),
                MySqlParam::Text(notify_recipients),
                MySqlParam::I64(if config.notify_critical { 1 } else { 0 }),
                MySqlParam::I64(if config.notify_security { 1 } else { 0 }),
                MySqlParam::I64(if config.notify_config_changes { 1 } else { 0 }),
                MySqlParam::I64(if config.notify_service_status { 1 } else { 0 }),
                MySqlParam::I64(if config.notify_resource_pressure {
                    1
                } else {
                    0
                }),
                MySqlParam::I64(if config.notify_connection_pressure {
                    1
                } else {
                    0
                }),
                MySqlParam::I64(config.notify_cooldown_seconds as i64),
                MySqlParam::I64(config.notify_cpu_threshold as i64),
                MySqlParam::I64(config.notify_ram_threshold as i64),
                MySqlParam::I64(config.notify_disk_threshold as i64),
                MySqlParam::I64(config.notify_connection_percent_threshold as i64),
            ]),
        )
        .await
        .map_err(mysql_error_to_sqlx)?;
        Ok(())
    }

    async fn insert_metric(
        &self,
        timestamp: &DateTime<Utc>,
        active_sessions: u64,
        total_sessions: u64,
        bandwidth: u64,
    ) -> Result<(), sqlx::Error> {
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        conn.exec_drop(
            r#"
            INSERT INTO metrics_snapshots (`timestamp`, active_sessions, total_sessions, bandwidth)
            VALUES (?, ?, ?, ?)
            "#,
            mysql_params(vec![
                MySqlParam::Text(timestamp.to_rfc3339()),
                MySqlParam::I64(active_sessions as i64),
                MySqlParam::I64(total_sessions as i64),
                MySqlParam::I64(bandwidth as i64),
            ]),
        )
        .await
        .map_err(mysql_error_to_sqlx)?;
        Ok(())
    }

    async fn query_metrics(
        &self,
        start: Option<&DateTime<Utc>>,
        limit: Option<u64>,
    ) -> Result<Vec<MetricsSnapshot>, sqlx::Error> {
        let mut sql = String::from(
            r#"
            SELECT `timestamp`, active_sessions, total_sessions, bandwidth
            FROM metrics_snapshots
            WHERE 1=1
            "#,
        );
        let mut params = Vec::new();
        if let Some(start_time) = start {
            sql.push_str(" AND `timestamp` >= ?");
            params.push(MySqlParam::Text(start_time.to_rfc3339()));
        }
        sql.push_str(" ORDER BY `timestamp` DESC");
        if let Some(limit_val) = limit {
            sql.push_str(" LIMIT ?");
            params.push(MySqlParam::I64(limit_val as i64));
        }

        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        let rows: Vec<MySqlMetricTuple> = conn
            .exec(sql, mysql_params(params))
            .await
            .map_err(mysql_error_to_sqlx)?;

        rows.into_iter()
            .map(
                |(timestamp, active_sessions, total_sessions, bandwidth)| MetricSnapshotRow {
                    timestamp,
                    active_sessions,
                    total_sessions,
                    bandwidth,
                },
            )
            .map(MetricSnapshotRow::into_metric)
            .collect()
    }

    async fn cleanup_old_metrics(&self, retention_hours: u64) -> Result<u64, sqlx::Error> {
        if retention_hours == 0 {
            return Ok(0);
        }

        let cutoff = Utc::now() - ChronoDuration::hours(retention_hours as i64);
        let mut conn = self.pool.get_conn().await.map_err(mysql_error_to_sqlx)?;
        conn.exec_drop(
            "DELETE FROM metrics_snapshots WHERE `timestamp` < ?",
            (cutoff.to_rfc3339(),),
        )
        .await
        .map_err(mysql_error_to_sqlx)?;
        Ok(conn.affected_rows())
    }

    async fn ensure_schema(conn: &mut mysql_async::Conn) -> Result<(), sqlx::Error> {
        for statement in mysql_schema_statements() {
            conn.query_drop(*statement)
                .await
                .map_err(mysql_error_to_sqlx)?;
        }

        for (table, index, ddl) in mysql_index_statements() {
            Self::ensure_index(conn, table, index, ddl).await?;
        }

        for (column, ddl) in mysql_smtp_columns() {
            if !Self::column_exists(conn, "smtp_config", column).await? {
                conn.query_drop(*ddl).await.map_err(mysql_error_to_sqlx)?;
            }
        }

        conn.query_drop(
            r#"
            INSERT INTO smtp_config (
                id, enabled, mode, host, port, from_address, from_name, username,
                password_encrypted, notify_recipients, notify_critical, notify_security,
                notify_config_changes, notify_service_status, notify_resource_pressure,
                notify_connection_pressure, notify_cooldown_seconds, notify_cpu_threshold,
                notify_ram_threshold, notify_disk_threshold, notify_connection_percent_threshold
            )
            VALUES (
                1, 0, 'starttls_auth', '', 587, '', NULL, NULL, NULL, '',
                0, 0, 0, 0, 0, 0, 3600, 85, 85, 90, 85
            )
            ON DUPLICATE KEY UPDATE id = VALUES(id)
            "#,
        )
        .await
        .map_err(mysql_error_to_sqlx)?;

        Ok(())
    }

    async fn ensure_index(
        conn: &mut mysql_async::Conn,
        table: &str,
        index: &str,
        ddl: &str,
    ) -> Result<(), sqlx::Error> {
        let exists: Option<(String,)> = conn
            .exec_first(
                r#"
                SELECT INDEX_NAME
                FROM information_schema.statistics
                WHERE table_schema = DATABASE() AND table_name = ? AND index_name = ?
                LIMIT 1
                "#,
                (table, index),
            )
            .await
            .map_err(mysql_error_to_sqlx)?;

        if exists.is_none() {
            conn.query_drop(ddl).await.map_err(mysql_error_to_sqlx)?;
        }

        Ok(())
    }

    async fn column_exists(
        conn: &mut mysql_async::Conn,
        table: &str,
        column: &str,
    ) -> Result<bool, sqlx::Error> {
        let exists: Option<(String,)> = conn
            .exec_first(
                r#"
                SELECT COLUMN_NAME
                FROM information_schema.columns
                WHERE table_schema = DATABASE() AND table_name = ? AND column_name = ?
                LIMIT 1
                "#,
                (table, column),
            )
            .await
            .map_err(mysql_error_to_sqlx)?;
        Ok(exists.is_some())
    }
}

impl DatabaseFlavor {
    fn from_url(url: &str) -> Result<Self, sqlx::Error> {
        let normalized = url.trim();
        let lower = normalized.to_ascii_lowercase();
        if lower.starts_with("sqlite:") {
            let options = SqliteConnectOptions::from_str(normalized)?;
            let filename = options.get_filename();
            let is_memory = SessionStore::is_in_memory_database(filename, normalized);
            let db_path = if is_memory {
                None
            } else {
                Some(filename.to_path_buf())
            };
            let connect_url = if is_memory {
                let encoded_filename = filename.to_string_lossy().replace(':', "%3A");
                format!("sqlite://{}?mode=memory&cache=shared", encoded_filename)
            } else {
                normalized.to_string()
            };
            Ok(Self::Sqlite {
                db_path,
                is_memory,
                connect_url,
            })
        } else if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
            Ok(Self::MariaDb)
        } else {
            Err(sqlx::Error::Configuration(
                format!(
                    "Unsupported session database URL: {}. Supported schemes: sqlite://..., mysql://..., mariadb://...",
                    normalized
                )
                .into(),
            ))
        }
    }

    fn sqlite_path(&self) -> Option<&Path> {
        match self {
            DatabaseFlavor::Sqlite {
                db_path: Some(path),
                ..
            } => Some(path),
            _ => None,
        }
    }

    fn is_sqlite(&self) -> bool {
        matches!(self, DatabaseFlavor::Sqlite { .. })
    }

    fn is_memory(&self) -> bool {
        matches!(
            self,
            DatabaseFlavor::Sqlite {
                is_memory: true,
                ..
            }
        )
    }

    fn connection_url<'a>(&'a self, original: &'a str) -> &'a str {
        match self {
            DatabaseFlavor::Sqlite { connect_url, .. } => connect_url,
            DatabaseFlavor::MariaDb => original,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ConnectionInfo, SessionProtocol, SessionStatus};
    use chrono::SecondsFormat;
    use std::net::{IpAddr, Ipv4Addr};

    use super::models::sanitize_duration;

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
        assert_eq!(results[0].user.as_ref(), "alice");

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

    #[test]
    fn parse_datetime_handles_rfc3339_with_timezone() {
        let ts = "2025-10-09T11:22:49.421595Z";
        let parsed = parse_datetime("start_time", ts).expect("rfc3339 timestamp");
        assert_eq!(
            parsed.to_rfc3339_opts(SecondsFormat::Micros, true),
            "2025-10-09T11:22:49.421595Z"
        );
    }

    #[test]
    fn parse_datetime_handles_legacy_timestamps_without_timezone() {
        let ts = "2025-10-09T11:22:49.421595";
        let parsed = parse_datetime("start_time", ts).expect("legacy timestamp");
        assert_eq!(
            parsed.to_rfc3339_opts(SecondsFormat::Micros, true),
            "2025-10-09T11:22:49.421595Z"
        );
    }

    #[test]
    fn sanitize_duration_drops_negative_values() {
        assert_eq!(sanitize_duration(Some(-1)), None);
        assert_eq!(sanitize_duration(Some(42)), Some(42));
        assert_eq!(sanitize_duration(None), None);
    }
}
