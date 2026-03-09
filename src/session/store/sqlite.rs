use super::{sqlx, DatabaseFlavor};
use chrono::Utc;
use sqlx_sqlite::SqlitePool;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub(super) async fn apply_migrations(
    pool: &SqlitePool,
    flavor: &DatabaseFlavor,
) -> Result<(), sqlx::Error> {
    let migrator = sqlx::migrate::Migrator::new(Path::new("./migrations"))
        .await
        .map_err(|e| sqlx::Error::Migrate(Box::new(e)))?;

    match migrator.run(pool).await {
        Ok(()) => Ok(()),
        Err(sqlx::migrate::MigrateError::VersionMismatch(version)) => {
            if repair_migration_checksum(pool, flavor, &migrator, version).await? {
                migrator
                    .run(pool)
                    .await
                    .map_err(|e| sqlx::Error::Migrate(Box::new(e)))
            } else {
                Err(sqlx::Error::Migrate(Box::new(
                    sqlx::migrate::MigrateError::VersionMismatch(version),
                )))
            }
        }
        Err(other) => Err(sqlx::Error::Migrate(Box::new(other))),
    }
}

async fn repair_migration_checksum(
    pool: &SqlitePool,
    flavor: &DatabaseFlavor,
    migrator: &sqlx::migrate::Migrator,
    version: i64,
) -> Result<bool, sqlx::Error> {
    if !flavor.is_sqlite() {
        return Ok(false);
    }

    let required = match version {
        9 => &[
            "notify_recipients",
            "notify_critical",
            "notify_security",
            "notify_config_changes",
            "notify_service_status",
        ][..],
        10 => &[
            "notify_cooldown_seconds",
            "notify_cpu_threshold",
            "notify_ram_threshold",
            "notify_disk_threshold",
            "notify_connection_percent_threshold",
            "notify_resource_pressure",
            "notify_connection_pressure",
        ][..],
        _ => return Ok(false),
    };

    let Some(migration) = migrator.iter().find(|m| m.version == version) else {
        return Ok(false);
    };

    if !smtp_columns_present(pool, required).await? {
        return Ok(false);
    }

    let updated = sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
        .bind(migration.checksum.as_ref())
        .bind(version)
        .execute(pool)
        .await?;

    if updated.rows_affected() == 0 {
        return Ok(false);
    }

    warn!(
        version,
        "Repaired SQLx migration checksum after detecting a modified migration"
    );
    Ok(true)
}

async fn smtp_columns_present(pool: &SqlitePool, required: &[&str]) -> Result<bool, sqlx::Error> {
    let columns: Vec<String> =
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info('smtp_config')")
            .fetch_all(pool)
            .await?;

    let columns: HashSet<String> = columns
        .into_iter()
        .map(|column| column.to_ascii_lowercase())
        .collect();

    Ok(required.iter().all(|name| columns.contains(*name)))
}

pub(super) async fn configure_journal_mode(pool: &SqlitePool) -> Result<(), sqlx::Error> {
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
        if let Err(e) = probe_wal_support(pool).await {
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

async fn probe_wal_support(pool: &SqlitePool) -> Result<(), sqlx::Error> {
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

pub(super) async fn verify_integrity(pool: &SqlitePool) -> Result<(), sqlx::Error> {
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

pub(super) fn preflight_database_file(path: &Path) -> Result<(), sqlx::Error> {
    if path == Path::new(":memory:") || !path.exists() {
        return Ok(());
    }

    let metadata = fs::metadata(path).map_err(sqlx::Error::Io)?;
    if metadata.len() < 16 {
        warn!(
            "Database file {:?} is too small to be valid, quarantining",
            path
        );
        quarantine_database_file(path)?;
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
        quarantine_database_file(path)?;
        return Ok(());
    }

    if &header != b"SQLite format 3\0" {
        warn!(
            "Invalid SQLite header detected in {:?}, quarantining corrupted file",
            path
        );
        quarantine_database_file(path)?;
    }

    Ok(())
}

pub(super) fn create_database_backup(path: &Path) -> Result<(), sqlx::Error> {
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

pub(super) fn quarantine_database_file(path: &Path) -> Result<(), sqlx::Error> {
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
