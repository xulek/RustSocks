use super::types::{Protocol as SessionProtocol, Session, SessionFilter, SessionStatus};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Utc};
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Any, AnyPool, FromRow, QueryBuilder};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Persistent storage for session history.
#[derive(Debug)]
pub struct SessionStore {
    pool: AnyPool,
    flavor: DatabaseFlavor,
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
            Self::preflight_database_file(path)?;

            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
                }
            }

            if !path.exists() {
                std::fs::File::create(path).map_err(sqlx::Error::Io)?;
            }

            Self::create_database_backup(path)?;
        }

        install_default_drivers();

        let connect_url = flavor.connection_url(database_url);
        let pool = AnyPoolOptions::new()
            .max_connections(5)
            .connect(connect_url)
            .await?;

        // Apply migrations shipped in the `migrations/` directory.
        if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
            return Err(sqlx::Error::Migrate(Box::new(e)));
        }

        if flavor.is_sqlite() {
            Self::configure_journal_mode(&pool).await?;

            // Optimize for performance with 500k+ rows (SQLite-only)
            sqlx::query("PRAGMA synchronous = FULL")
                .execute(&pool)
                .await?;

            sqlx::query("PRAGMA cache_size = -64000")
                .execute(&pool)
                .await?;

            sqlx::query("PRAGMA page_size = 8192")
                .execute(&pool)
                .await?;

            sqlx::query("PRAGMA mmap_size = 268435456")
                .execute(&pool)
                .await?;

            sqlx::query("PRAGMA optimize").execute(&pool).await?;

            info!(
                "SQLite optimizations enabled: safe journaling, 64MB cache, 8KB pages, 256MB mmap"
            );

            if let Err(e) = Self::verify_integrity(&pool).await {
                warn!(
                    error = %e,
                    "SQLite integrity check failed"
                );
                if allow_reset {
                    pool.close().await;
                    if flavor.is_memory() {
                        warn!("Reinitializing in-memory database after failed integrity check");
                    } else if let Some(path) = flavor.sqlite_path() {
                        Self::quarantine_database_file(path)?;
                        warn!("Corrupted database quarantined; recreating a fresh database");
                    }
                    return Ok(None);
                } else {
                    return Err(e);
                }
            }
        }

        Ok(Some(Self { pool, flavor }))
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

    /// Mark all active sessions as closed (called on server startup to clean up stale sessions).
    pub async fn close_all_active_sessions(&self) -> Result<u64, sqlx::Error> {
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
        .execute(&self.pool)
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

    /// Access underlying connection pool.
    pub fn pool(&self) -> &AnyPool {
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
        let mut builder = QueryBuilder::<Any>::new(
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
            _ => "start_time", // default fallback
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
        let rows = query.fetch_all(&self.pool).await?;

        rows.into_iter().map(SessionRow::into_session).collect()
    }

    pub async fn count_sessions(&self, filter: &SessionFilter) -> Result<u64, sqlx::Error> {
        // If filter is mostly empty and table is large, use approximate count
        let is_simple_filter = filter.user.is_none()
            && filter.dest_ip.is_none()
            && filter.status.is_none()
            && filter.start_after.is_none();

        if is_simple_filter {
            // Use fast approximate count for unfiltered queries
            return self.approximate_total_sessions().await;
        }

        let mut builder = QueryBuilder::<Any>::new(
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

    /// Get approximate total count of sessions (fast for large tables)
    ///
    /// Uses SQLite's internal statistics instead of scanning all rows.
    /// Much faster than COUNT(*) for tables with 100k+ rows.
    pub async fn approximate_total_sessions(&self) -> Result<u64, sqlx::Error> {
        if self.flavor.is_sqlite() {
            // Try to get approximate count from sqlite_stat1 (updated by ANALYZE)
            let result: Option<(i64,)> = sqlx::query_as(
                r#"
                SELECT stat FROM sqlite_stat1
                WHERE tbl = 'sessions' AND idx IS NULL
                "#,
            )
            .fetch_optional(&self.pool)
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
                .fetch_optional(&self.pool)
                .await?;

            let approx = max_rowid.unwrap_or(0) as u64;
            debug!(approx = approx, "Using max ROWID as approximate count");
            return Ok(approx);
        }

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as u64)
    }

    pub async fn existing_session_ids(&self, ids: &[Uuid]) -> Result<HashSet<Uuid>, sqlx::Error> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut builder = QueryBuilder::<Any>::new(
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
        let mut builder = QueryBuilder::<Any>::new(
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

    fn push_filters(builder: &mut QueryBuilder<'_, Any>, filter: &SessionFilter) {
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
            WHERE start_time < ?;
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
            WHERE timestamp < ?;
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

impl SessionStore {
    async fn configure_journal_mode(pool: &AnyPool) -> Result<(), sqlx::Error> {
        let wal_enabled = match sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
            .fetch_one(pool)
            .await
        {
            Ok(mode) => mode.eq_ignore_ascii_case("wal"),
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to set WAL journal mode, falling back to default"
                );
                false
            }
        };

        if wal_enabled {
            if let Err(e) = Self::probe_wal_support(pool).await {
                warn!(
                    error = %e,
                    "SQLite filesystem does not support WAL shared memory, falling back to DELETE journal mode"
                );
                sqlx::query("PRAGMA journal_mode = DELETE")
                    .execute(pool)
                    .await?;
                info!("SQLite journal mode set to DELETE");
            } else {
                info!("SQLite WAL mode enabled");
                return Ok(());
            }
        } else {
            sqlx::query("PRAGMA journal_mode = DELETE")
                .execute(pool)
                .await?;
            info!("SQLite journal mode set to DELETE");
        }

        Ok(())
    }

    async fn probe_wal_support(pool: &AnyPool) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS __wal_probe(value INTEGER)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DROP TABLE IF EXISTS __wal_probe")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn verify_integrity(pool: &AnyPool) -> Result<(), sqlx::Error> {
        let result: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(pool)
            .await?;
        if result.trim().eq_ignore_ascii_case("ok") {
            Ok(())
        } else {
            Err(sqlx::Error::Protocol(format!(
                "SQLite integrity check failed: {}",
                result
            )))
        }
    }

    fn preflight_database_file(path: &Path) -> Result<(), sqlx::Error> {
        if path == Path::new(":memory:") || !path.exists() {
            return Ok(());
        }

        let metadata = fs::metadata(path).map_err(sqlx::Error::Io)?;
        if metadata.len() < 16 {
            warn!(
                "Database file {:?} is too small to be valid, quarantining",
                path
            );
            Self::quarantine_database_file(path)?;
            return Ok(());
        }

        let mut file = fs::File::open(path).map_err(sqlx::Error::Io)?;
        let mut header = [0u8; 16];
        if let Err(e) = file.read_exact(&mut header) {
            warn!(
                error = %e,
                "Failed to read SQLite header from {:?}, quarantining",
                path
            );
            Self::quarantine_database_file(path)?;
            return Ok(());
        }

        if &header != b"SQLite format 3\0" {
            warn!(
                "Invalid SQLite header detected in {:?}, quarantining corrupted file",
                path
            );
            Self::quarantine_database_file(path)?;
        }

        Ok(())
    }

    fn create_database_backup(path: &Path) -> Result<(), sqlx::Error> {
        if path == Path::new(":memory:") || !path.exists() {
            return Ok(());
        }

        let metadata = fs::metadata(path).map_err(sqlx::Error::Io)?;
        if metadata.len() == 0 {
            return Ok(());
        }

        let timestamp = Utc::now().format("%Y%m%d%H%M%S");
        let backup_name = format!(
            "{}.bak.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sessions.db"),
            timestamp
        );
        let backup_path = path
            .parent()
            .map(|parent| parent.join(&backup_name))
            .unwrap_or_else(|| PathBuf::from(&backup_name));

        fs::copy(path, &backup_path).map_err(sqlx::Error::Io)?;
        info!(
            original = ?path,
            backup = ?backup_path,
            "Created SQLite safety backup"
        );
        Ok(())
    }

    fn quarantine_database_file(path: &Path) -> Result<(), sqlx::Error> {
        if path == Path::new(":memory:") || !path.exists() {
            return Ok(());
        }

        let timestamp = Utc::now().format("%Y%m%d%H%M%S");
        let quarantine_name = format!(
            "{}.corrupt.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("sessions.db"),
            timestamp
        );
        let quarantine_path = path
            .parent()
            .map(|parent| parent.join(&quarantine_name))
            .unwrap_or_else(|| PathBuf::from(&quarantine_name));

        fs::rename(path, &quarantine_path).map_err(sqlx::Error::Io)?;

        let wal_path = path.with_extension("db-wal");
        if wal_path.exists() {
            let _ = fs::remove_file(&wal_path);
        }
        let shm_path = path.with_extension("db-shm");
        if shm_path.exists() {
            let _ = fs::remove_file(&shm_path);
        }

        warn!(
            original = ?path,
            quarantine = ?quarantine_path,
            "Quarantined corrupted SQLite file"
        );
        Ok(())
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
            user: self.user.into(),
            start_time,
            end_time,
            duration_secs: sanitize_duration(self.duration_secs),
            source_ip,
            source_port: self.source_port as u16,
            dest_ip: self.dest_ip.into(),
            dest_port: self.dest_port as u16,
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
            user: Cow::Borrowed(session.user.as_ref()),
            start_time: session.start_time.to_rfc3339(),
            end_time: session.end_time.map(|dt| dt.to_rfc3339()),
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
            acl_rule_matched: session.acl_rule_matched.as_ref().map(|s| s.to_string()),
            acl_decision: Cow::Borrowed(session.acl_decision.as_ref()),
        }
    }
}

fn parse_datetime(field: &str, value: &str) -> Result<DateTime<Utc>, sqlx::Error> {
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

fn sanitize_duration(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| if v >= 0 { Some(v as u64) } else { None })
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
    use chrono::SecondsFormat;
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
