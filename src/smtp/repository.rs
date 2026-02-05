use sqlx::{Any, Pool};
use tracing::debug;

use super::encryption::{decrypt_password, encrypt_password};
use super::types::{format_recipients, parse_recipients, SmtpConfig, SmtpMode};

pub struct SmtpRepository {
    pool: Pool<Any>,
    api_token: Option<String>,
}

#[derive(sqlx::FromRow)]
struct SmtpConfigRow {
    enabled: i32,
    mode: String,
    host: String,
    port: i32,
    from_address: String,
    from_name: Option<String>,
    username: Option<String>,
    password_encrypted: Option<String>,
    notify_recipients: Option<String>,
    notify_critical: i32,
    notify_security: i32,
    notify_config_changes: i32,
    notify_service_status: i32,
    notify_resource_pressure: i32,
    notify_connection_pressure: i32,
    notify_cooldown_seconds: i32,
    notify_cpu_threshold: i32,
    notify_ram_threshold: i32,
    notify_disk_threshold: i32,
    notify_connection_percent_threshold: i32,
}

impl SmtpRepository {
    pub fn new(pool: Pool<Any>, api_token: Option<String>) -> Self {
        Self { pool, api_token }
    }

    pub async fn get_config(&self) -> Result<SmtpConfig, String> {
        let row: Option<SmtpConfigRow> = sqlx::query_as(
            "SELECT enabled, mode, host, port, from_address, from_name, username, password_encrypted, notify_recipients, notify_critical, notify_security, notify_config_changes, notify_service_status, notify_resource_pressure, notify_connection_pressure, notify_cooldown_seconds, notify_cpu_threshold, notify_ram_threshold, notify_disk_threshold, notify_connection_percent_threshold FROM smtp_config WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        match row {
            Some(row) => {
                let mode: SmtpMode = row.mode.parse().unwrap_or_default();
                let has_password = row.password_encrypted.is_some();
                let notify_recipients =
                    parse_recipients(row.notify_recipients.as_deref().unwrap_or(""));

                let password = match (&row.password_encrypted, &self.api_token) {
                    (Some(enc), Some(token)) => decrypt_password(enc, token).ok(),
                    _ => None,
                };

                Ok(SmtpConfig {
                    enabled: row.enabled != 0,
                    mode,
                    host: row.host,
                    port: row.port as u16,
                    from_address: row.from_address,
                    from_name: row.from_name,
                    username: row.username,
                    password,
                    has_password,
                    notify_recipients,
                    notify_critical: row.notify_critical != 0,
                    notify_security: row.notify_security != 0,
                    notify_config_changes: row.notify_config_changes != 0,
                    notify_service_status: row.notify_service_status != 0,
                    notify_resource_pressure: row.notify_resource_pressure != 0,
                    notify_connection_pressure: row.notify_connection_pressure != 0,
                    notify_cooldown_seconds: row.notify_cooldown_seconds.max(0) as u64,
                    notify_cpu_threshold: row.notify_cpu_threshold.clamp(1, 100) as u8,
                    notify_ram_threshold: row.notify_ram_threshold.clamp(1, 100) as u8,
                    notify_disk_threshold: row.notify_disk_threshold.clamp(1, 100) as u8,
                    notify_connection_percent_threshold: row
                        .notify_connection_percent_threshold
                        .clamp(1, 100)
                        as u8,
                })
            }
            None => Ok(SmtpConfig::default()),
        }
    }

    pub async fn save_config(
        &self,
        config: &SmtpConfig,
        new_password: Option<&str>,
    ) -> Result<(), String> {
        let password_encrypted = match new_password {
            Some(pwd) if !pwd.is_empty() => {
                let token = self
                    .api_token
                    .as_ref()
                    .ok_or("API token required for password encryption")?;
                Some(encrypt_password(pwd, token)?)
            }
            _ => None,
        };
        let notify_recipients = format_recipients(&config.notify_recipients);

        let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM smtp_config WHERE id = 1")
            .fetch_one(&self.pool)
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
                .execute(&self.pool)
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
                .execute(&self.pool)
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
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to insert config: {}", e))?;
        }

        debug!("SMTP configuration saved");
        Ok(())
    }
}
