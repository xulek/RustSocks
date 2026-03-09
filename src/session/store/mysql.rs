use super::models::{SessionParams, SessionRow, SmtpConfigRow};
use super::{sqlx, SessionFilter};
use mysql_async::{Params as MySqlParams, Row as MySqlRow, Value as MySqlValue};

#[derive(Debug, Clone)]
pub(super) enum MySqlParam {
    Text(String),
    OptText(Option<String>),
    I64(i64),
    OptI64(Option<i64>),
}

impl MySqlParam {
    fn into_value(self) -> MySqlValue {
        match self {
            Self::Text(value) => MySqlValue::Bytes(value.into_bytes()),
            Self::OptText(Some(value)) => MySqlValue::Bytes(value.into_bytes()),
            Self::OptText(None) => MySqlValue::NULL,
            Self::I64(value) => MySqlValue::Int(value),
            Self::OptI64(Some(value)) => MySqlValue::Int(value),
            Self::OptI64(None) => MySqlValue::NULL,
        }
    }
}

pub(super) type MySqlMetricTuple = (String, i64, i64, i64);

pub(super) fn mysql_row_to_session_row(mut row: MySqlRow) -> SessionRow {
    SessionRow {
        session_id: mysql_take_required(&mut row, "session_id"),
        user: mysql_take_required(&mut row, "user"),
        start_time: mysql_take_required(&mut row, "start_time"),
        end_time: mysql_take_optional(&mut row, "end_time"),
        duration_secs: mysql_take_optional(&mut row, "duration_secs"),
        source_ip: mysql_take_required(&mut row, "source_ip"),
        source_port: mysql_take_required(&mut row, "source_port"),
        dest_ip: mysql_take_required(&mut row, "dest_ip"),
        dest_port: mysql_take_required(&mut row, "dest_port"),
        protocol: mysql_take_required(&mut row, "protocol"),
        bytes_sent: mysql_take_required(&mut row, "bytes_sent"),
        bytes_received: mysql_take_required(&mut row, "bytes_received"),
        packets_sent: mysql_take_required(&mut row, "packets_sent"),
        packets_received: mysql_take_required(&mut row, "packets_received"),
        status: mysql_take_required(&mut row, "status"),
        close_reason: mysql_take_optional(&mut row, "close_reason"),
        acl_rule_matched: mysql_take_optional(&mut row, "acl_rule_matched"),
        acl_decision: mysql_take_required(&mut row, "acl_decision"),
    }
}

pub(super) fn mysql_row_to_smtp_config_row(mut row: MySqlRow) -> SmtpConfigRow {
    SmtpConfigRow {
        enabled: mysql_take_required::<i64>(&mut row, "enabled") as i32,
        mode: mysql_take_required(&mut row, "mode"),
        host: mysql_take_required(&mut row, "host"),
        port: mysql_take_required::<i64>(&mut row, "port") as i32,
        from_address: mysql_take_required(&mut row, "from_address"),
        from_name: mysql_take_optional(&mut row, "from_name"),
        username: mysql_take_optional(&mut row, "username"),
        password_encrypted: mysql_take_optional(&mut row, "password_encrypted"),
        notify_recipients: mysql_take_optional(&mut row, "notify_recipients"),
        notify_critical: mysql_take_required::<i64>(&mut row, "notify_critical") as i32,
        notify_security: mysql_take_required::<i64>(&mut row, "notify_security") as i32,
        notify_config_changes: mysql_take_required::<i64>(&mut row, "notify_config_changes") as i32,
        notify_service_status: mysql_take_required::<i64>(&mut row, "notify_service_status") as i32,
        notify_resource_pressure: mysql_take_required::<i64>(&mut row, "notify_resource_pressure")
            as i32,
        notify_connection_pressure: mysql_take_required::<i64>(
            &mut row,
            "notify_connection_pressure",
        ) as i32,
        notify_cooldown_seconds: mysql_take_required::<i64>(&mut row, "notify_cooldown_seconds")
            as i32,
        notify_cpu_threshold: mysql_take_required::<i64>(&mut row, "notify_cpu_threshold") as i32,
        notify_ram_threshold: mysql_take_required::<i64>(&mut row, "notify_ram_threshold") as i32,
        notify_disk_threshold: mysql_take_required::<i64>(&mut row, "notify_disk_threshold") as i32,
        notify_connection_percent_threshold: mysql_take_required::<i64>(
            &mut row,
            "notify_connection_percent_threshold",
        ) as i32,
    }
}

fn mysql_take_required<T>(row: &mut MySqlRow, column: &str) -> T
where
    T: mysql_async::prelude::FromValue,
{
    match row.take_opt::<T, _>(column) {
        Some(Ok(value)) => value,
        Some(Err(err)) => panic!("failed to decode mysql column {column}: {err}"),
        None => panic!("missing mysql column {column}"),
    }
}

fn mysql_take_optional<T>(row: &mut MySqlRow, column: &str) -> Option<T>
where
    T: mysql_async::prelude::FromValue,
{
    match row.take_opt::<Option<T>, _>(column) {
        Some(Ok(value)) => value,
        Some(Err(err)) => panic!("failed to decode mysql column {column}: {err}"),
        None => None,
    }
}

pub(super) fn mysql_params(params: Vec<MySqlParam>) -> MySqlParams {
    MySqlParams::Positional(params.into_iter().map(MySqlParam::into_value).collect())
}

pub(super) fn mysql_session_params(params: SessionParams<'_>) -> MySqlParams {
    mysql_params(vec![
        MySqlParam::Text(params.session_id.into_owned()),
        MySqlParam::Text(params.user.into_owned()),
        MySqlParam::Text(params.start_time),
        MySqlParam::OptText(params.end_time),
        MySqlParam::OptI64(params.duration_secs),
        MySqlParam::Text(params.source_ip.into_owned()),
        MySqlParam::I64(params.source_port),
        MySqlParam::Text(params.dest_ip.into_owned()),
        MySqlParam::I64(params.dest_port),
        MySqlParam::Text(params.protocol.into_owned()),
        MySqlParam::I64(params.bytes_sent),
        MySqlParam::I64(params.bytes_received),
        MySqlParam::I64(params.packets_sent),
        MySqlParam::I64(params.packets_received),
        MySqlParam::Text(params.status.into_owned()),
        MySqlParam::OptText(params.close_reason),
        MySqlParam::OptText(params.acl_rule_matched),
        MySqlParam::Text(params.acl_decision.into_owned()),
    ])
}

pub(super) fn push_mysql_filters(
    sql: &mut String,
    params: &mut Vec<MySqlParam>,
    filter: &SessionFilter,
) {
    if let Some(user) = &filter.user {
        sql.push_str(" AND `user` = ?");
        params.push(MySqlParam::Text(user.clone()));
    }

    if let Some(status) = &filter.status {
        sql.push_str(" AND status = ?");
        params.push(MySqlParam::Text(status.as_str().to_string()));
    }

    if let Some(start_after) = filter.start_after {
        sql.push_str(" AND start_time >= ?");
        params.push(MySqlParam::Text(start_after.to_rfc3339()));
    }

    if let Some(start_before) = filter.start_before {
        sql.push_str(" AND start_time <= ?");
        params.push(MySqlParam::Text(start_before.to_rfc3339()));
    }

    if let Some(dest_ip) = &filter.dest_ip {
        sql.push_str(" AND dest_ip = ?");
        params.push(MySqlParam::Text(dest_ip.clone()));
    }

    if let Some(min_duration) = filter.min_duration_secs {
        sql.push_str(" AND duration_secs IS NOT NULL AND duration_secs >= ?");
        params.push(MySqlParam::I64(min_duration as i64));
    }

    if let Some(min_bytes) = filter.min_bytes {
        sql.push_str(" AND (bytes_sent + bytes_received) >= ?");
        params.push(MySqlParam::I64(min_bytes as i64));
    }
}

pub(super) fn mysql_sort_column(column: Option<&str>) -> &'static str {
    match column.unwrap_or("start_time") {
        "user" => "`user`",
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
    }
}

pub(super) fn sort_direction(direction: Option<&str>) -> &'static str {
    if direction.unwrap_or("desc").eq_ignore_ascii_case("asc") {
        "ASC"
    } else {
        "DESC"
    }
}

pub(super) fn mysql_upsert_session_sql() -> &'static str {
    r#"
    INSERT INTO sessions (
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
    )
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON DUPLICATE KEY UPDATE
        `user` = VALUES(`user`),
        start_time = VALUES(start_time),
        end_time = VALUES(end_time),
        duration_secs = VALUES(duration_secs),
        source_ip = VALUES(source_ip),
        source_port = VALUES(source_port),
        dest_ip = VALUES(dest_ip),
        dest_port = VALUES(dest_port),
        protocol = VALUES(protocol),
        bytes_sent = VALUES(bytes_sent),
        bytes_received = VALUES(bytes_received),
        packets_sent = VALUES(packets_sent),
        packets_received = VALUES(packets_received),
        status = VALUES(status),
        close_reason = VALUES(close_reason),
        acl_rule_matched = VALUES(acl_rule_matched),
        acl_decision = VALUES(acl_decision)
    "#
}

pub(super) fn mysql_schema_statements() -> &'static [&'static str] {
    &[
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            session_id VARCHAR(36) PRIMARY KEY,
            `user` VARCHAR(255) NOT NULL,
            start_time VARCHAR(64) NOT NULL,
            end_time VARCHAR(64) NULL,
            duration_secs BIGINT NULL,
            source_ip VARCHAR(255) NOT NULL,
            source_port INT NOT NULL,
            dest_ip VARCHAR(255) NOT NULL,
            dest_port INT NOT NULL,
            protocol VARCHAR(16) NOT NULL,
            bytes_sent BIGINT NOT NULL DEFAULT 0,
            bytes_received BIGINT NOT NULL DEFAULT 0,
            packets_sent BIGINT NOT NULL DEFAULT 0,
            packets_received BIGINT NOT NULL DEFAULT 0,
            status VARCHAR(64) NOT NULL,
            close_reason TEXT NULL,
            acl_rule_matched TEXT NULL,
            acl_decision VARCHAR(64) NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS metrics_snapshots (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            `timestamp` VARCHAR(64) NOT NULL,
            active_sessions BIGINT NOT NULL,
            total_sessions BIGINT NOT NULL,
            bandwidth BIGINT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS smtp_config (
            id BIGINT PRIMARY KEY,
            enabled TINYINT(1) NOT NULL DEFAULT 0,
            mode VARCHAR(64) NOT NULL DEFAULT 'starttls_auth',
            host VARCHAR(255) NOT NULL DEFAULT '',
            port INT NOT NULL DEFAULT 587,
            from_address VARCHAR(255) NOT NULL DEFAULT '',
            from_name VARCHAR(255) NULL,
            username VARCHAR(255) NULL,
            password_encrypted TEXT NULL,
            notify_recipients TEXT NOT NULL DEFAULT '',
            notify_critical TINYINT(1) NOT NULL DEFAULT 0,
            notify_security TINYINT(1) NOT NULL DEFAULT 0,
            notify_config_changes TINYINT(1) NOT NULL DEFAULT 0,
            notify_service_status TINYINT(1) NOT NULL DEFAULT 0,
            notify_resource_pressure TINYINT(1) NOT NULL DEFAULT 0,
            notify_connection_pressure TINYINT(1) NOT NULL DEFAULT 0,
            notify_cooldown_seconds INT NOT NULL DEFAULT 3600,
            notify_cpu_threshold INT NOT NULL DEFAULT 85,
            notify_ram_threshold INT NOT NULL DEFAULT 85,
            notify_disk_threshold INT NOT NULL DEFAULT 90,
            notify_connection_percent_threshold INT NOT NULL DEFAULT 85,
            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS telemetry_events (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            `timestamp` VARCHAR(64) NOT NULL,
            severity VARCHAR(16) NOT NULL,
            category VARCHAR(64) NOT NULL,
            message TEXT NOT NULL,
            details TEXT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS aggregated_metrics (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            `timestamp` VARCHAR(64) NOT NULL,
            period_minutes INT NOT NULL DEFAULT 5,
            total_connections BIGINT NOT NULL DEFAULT 0,
            active_connections BIGINT NOT NULL DEFAULT 0,
            failed_connections BIGINT NOT NULL DEFAULT 0,
            avg_latency_ms DOUBLE NOT NULL DEFAULT 0,
            p50_latency_ms DOUBLE NOT NULL DEFAULT 0,
            p95_latency_ms DOUBLE NOT NULL DEFAULT 0,
            p99_latency_ms DOUBLE NOT NULL DEFAULT 0,
            max_latency_ms DOUBLE NOT NULL DEFAULT 0,
            bytes_sent BIGINT NOT NULL DEFAULT 0,
            bytes_received BIGINT NOT NULL DEFAULT 0,
            pool_hits BIGINT NOT NULL DEFAULT 0,
            pool_misses BIGINT NOT NULL DEFAULT 0,
            pool_hit_rate DOUBLE NOT NULL DEFAULT 0,
            pool_idle_connections BIGINT NOT NULL DEFAULT 0,
            pool_in_use_connections BIGINT NOT NULL DEFAULT 0,
            auth_failures BIGINT NOT NULL DEFAULT 0,
            connection_timeouts BIGINT NOT NULL DEFAULT 0,
            upstream_errors BIGINT NOT NULL DEFAULT 0,
            acl_denials BIGINT NOT NULL DEFAULT 0,
            memory_usage_bytes BIGINT NULL,
            cpu_usage_percent DOUBLE NULL,
            open_file_descriptors BIGINT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS alert_thresholds (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            metric_name VARCHAR(128) NOT NULL UNIQUE,
            display_name VARCHAR(255) NOT NULL,
            category VARCHAR(64) NOT NULL,
            warning_threshold DOUBLE NULL,
            error_threshold DOUBLE NULL,
            comparison VARCHAR(8) NOT NULL DEFAULT 'gt',
            enabled TINYINT(1) NOT NULL DEFAULT 1,
            unit VARCHAR(32) NULL,
            description TEXT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS alert_history (
            id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            metric_name VARCHAR(128) NOT NULL,
            severity VARCHAR(16) NOT NULL,
            current_value DOUBLE NOT NULL,
            threshold_value DOUBLE NOT NULL,
            message TEXT NOT NULL,
            triggered_at VARCHAR(64) NOT NULL,
            resolved_at VARCHAR(64) NULL,
            acknowledged TINYINT(1) NOT NULL DEFAULT 0,
            acknowledged_at VARCHAR(64) NULL,
            acknowledged_by VARCHAR(255) NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
        "#,
        r#"
        INSERT IGNORE INTO alert_thresholds (
            metric_name, display_name, category, warning_threshold, error_threshold, comparison, unit, description
        ) VALUES
            ('avg_latency_ms', 'Average Latency', 'performance', 100, 500, 'gt', 'ms', 'Average connection latency'),
            ('p95_latency_ms', 'P95 Latency', 'performance', 200, 1000, 'gt', 'ms', '95th percentile latency'),
            ('p99_latency_ms', 'P99 Latency', 'performance', 500, 2000, 'gt', 'ms', '99th percentile latency'),
            ('error_rate', 'Error Rate', 'errors', 5, 15, 'gt', '%', 'Percentage of failed connections'),
            ('auth_failure_rate', 'Auth Failure Rate', 'errors', 10, 25, 'gt', '%', 'Authentication failure rate'),
            ('timeout_rate', 'Timeout Rate', 'errors', 5, 10, 'gt', '%', 'Connection timeout rate'),
            ('memory_usage_percent', 'Memory Usage', 'resources', 70, 90, 'gt', '%', 'Memory usage percentage'),
            ('cpu_usage_percent', 'CPU Usage', 'resources', 70, 90, 'gt', '%', 'CPU usage percentage'),
            ('fd_usage_percent', 'File Descriptors', 'resources', 70, 90, 'gt', '%', 'File descriptor usage percentage'),
            ('active_connections_percent', 'Active Connections', 'limits', 70, 90, 'gt', '%', 'Percentage of max connections in use'),
            ('pool_utilization', 'Pool Utilization', 'limits', 80, 95, 'gt', '%', 'Connection pool utilization'),
            ('pool_hit_rate', 'Pool Hit Rate', 'limits', 50, 30, 'lt', '%', 'Connection pool hit rate (lower is worse)')
        "#,
    ]
}

pub(super) fn mysql_index_statements() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("sessions", "idx_sessions_user", "CREATE INDEX idx_sessions_user ON sessions (`user`)"),
        ("sessions", "idx_sessions_start_time", "CREATE INDEX idx_sessions_start_time ON sessions (start_time)"),
        ("sessions", "idx_sessions_dest_ip", "CREATE INDEX idx_sessions_dest_ip ON sessions (dest_ip)"),
        ("sessions", "idx_sessions_status", "CREATE INDEX idx_sessions_status ON sessions (status)"),
        ("sessions", "idx_sessions_user_start", "CREATE INDEX idx_sessions_user_start ON sessions (`user`, start_time)"),
        ("sessions", "idx_sessions_status_start", "CREATE INDEX idx_sessions_status_start ON sessions (status, start_time)"),
        ("sessions", "idx_sessions_dest_user", "CREATE INDEX idx_sessions_dest_user ON sessions (dest_ip, `user`)"),
        ("sessions", "idx_sessions_duration_order", "CREATE INDEX idx_sessions_duration_order ON sessions (duration_secs, session_id)"),
        ("sessions", "idx_sessions_acl_decision_start", "CREATE INDEX idx_sessions_acl_decision_start ON sessions (acl_decision, start_time)"),
        ("sessions", "idx_sessions_user_status_start", "CREATE INDEX idx_sessions_user_status_start ON sessions (`user`, status, start_time)"),
        ("sessions", "idx_sessions_bytes_sent", "CREATE INDEX idx_sessions_bytes_sent ON sessions (bytes_sent)"),
        ("sessions", "idx_sessions_bytes_received", "CREATE INDEX idx_sessions_bytes_received ON sessions (bytes_received)"),
        ("sessions", "idx_sessions_source_ip", "CREATE INDEX idx_sessions_source_ip ON sessions (source_ip, session_id)"),
        ("sessions", "idx_sessions_protocol", "CREATE INDEX idx_sessions_protocol ON sessions (protocol)"),
        ("sessions", "idx_sessions_acl_decision", "CREATE INDEX idx_sessions_acl_decision ON sessions (acl_decision)"),
        ("metrics_snapshots", "idx_metrics_timestamp", "CREATE INDEX idx_metrics_timestamp ON metrics_snapshots (`timestamp`)"),
        ("metrics_snapshots", "idx_metrics_created_at", "CREATE INDEX idx_metrics_created_at ON metrics_snapshots (created_at)"),
        ("telemetry_events", "idx_telemetry_timestamp", "CREATE INDEX idx_telemetry_timestamp ON telemetry_events (`timestamp`)"),
        ("telemetry_events", "idx_telemetry_severity", "CREATE INDEX idx_telemetry_severity ON telemetry_events (severity)"),
        ("telemetry_events", "idx_telemetry_category", "CREATE INDEX idx_telemetry_category ON telemetry_events (category)"),
        ("telemetry_events", "idx_telemetry_time_severity", "CREATE INDEX idx_telemetry_time_severity ON telemetry_events (`timestamp`, severity)"),
        ("telemetry_events", "idx_telemetry_created_at", "CREATE INDEX idx_telemetry_created_at ON telemetry_events (created_at)"),
        ("aggregated_metrics", "idx_agg_metrics_timestamp", "CREATE INDEX idx_agg_metrics_timestamp ON aggregated_metrics (`timestamp`)"),
        ("aggregated_metrics", "idx_agg_metrics_period", "CREATE INDEX idx_agg_metrics_period ON aggregated_metrics (period_minutes, `timestamp`)"),
        ("aggregated_metrics", "idx_agg_metrics_created_at", "CREATE INDEX idx_agg_metrics_created_at ON aggregated_metrics (created_at)"),
        ("alert_history", "idx_alert_history_active", "CREATE INDEX idx_alert_history_active ON alert_history (resolved_at)"),
        ("alert_history", "idx_alert_history_triggered", "CREATE INDEX idx_alert_history_triggered ON alert_history (triggered_at)"),
        ("alert_history", "idx_alert_history_metric", "CREATE INDEX idx_alert_history_metric ON alert_history (metric_name, triggered_at)"),
    ]
}

pub(super) fn mysql_smtp_columns() -> &'static [(&'static str, &'static str)] {
    &[
        ("notify_recipients", "ALTER TABLE smtp_config ADD COLUMN notify_recipients TEXT NOT NULL DEFAULT ''"),
        ("notify_critical", "ALTER TABLE smtp_config ADD COLUMN notify_critical TINYINT(1) NOT NULL DEFAULT 0"),
        ("notify_security", "ALTER TABLE smtp_config ADD COLUMN notify_security TINYINT(1) NOT NULL DEFAULT 0"),
        ("notify_config_changes", "ALTER TABLE smtp_config ADD COLUMN notify_config_changes TINYINT(1) NOT NULL DEFAULT 0"),
        ("notify_service_status", "ALTER TABLE smtp_config ADD COLUMN notify_service_status TINYINT(1) NOT NULL DEFAULT 0"),
        ("notify_cooldown_seconds", "ALTER TABLE smtp_config ADD COLUMN notify_cooldown_seconds INT NOT NULL DEFAULT 3600"),
        ("notify_cpu_threshold", "ALTER TABLE smtp_config ADD COLUMN notify_cpu_threshold INT NOT NULL DEFAULT 85"),
        ("notify_ram_threshold", "ALTER TABLE smtp_config ADD COLUMN notify_ram_threshold INT NOT NULL DEFAULT 85"),
        ("notify_disk_threshold", "ALTER TABLE smtp_config ADD COLUMN notify_disk_threshold INT NOT NULL DEFAULT 90"),
        ("notify_connection_percent_threshold", "ALTER TABLE smtp_config ADD COLUMN notify_connection_percent_threshold INT NOT NULL DEFAULT 85"),
        ("notify_resource_pressure", "ALTER TABLE smtp_config ADD COLUMN notify_resource_pressure TINYINT(1) NOT NULL DEFAULT 0"),
        ("notify_connection_pressure", "ALTER TABLE smtp_config ADD COLUMN notify_connection_pressure TINYINT(1) NOT NULL DEFAULT 0"),
    ]
}

pub(super) fn mysql_error_to_sqlx(error: mysql_async::Error) -> sqlx::Error {
    sqlx::Error::Protocol(error.to_string())
}
