use sqlx::{Any, Pool};
use tracing::debug;

use super::encryption::{decrypt_password, encrypt_password};
use super::types::{SmtpConfig, SmtpMode};

pub struct SmtpRepository {
    pool: Pool<Any>,
    api_token: Option<String>,
}

impl SmtpRepository {
    pub fn new(pool: Pool<Any>, api_token: Option<String>) -> Self {
        Self { pool, api_token }
    }

    pub async fn get_config(&self) -> Result<SmtpConfig, String> {
        let row: Option<(
            i32,
            String,
            String,
            i32,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT enabled, mode, host, port, from_address, from_name, username, password_encrypted FROM smtp_config WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        match row {
            Some((
                enabled,
                mode_str,
                host,
                port,
                from_address,
                from_name,
                username,
                password_encrypted,
            )) => {
                let mode: SmtpMode = mode_str.parse().unwrap_or_default();
                let has_password = password_encrypted.is_some();

                let password = match (&password_encrypted, &self.api_token) {
                    (Some(enc), Some(token)) => decrypt_password(enc, token).ok(),
                    _ => None,
                };

                Ok(SmtpConfig {
                    enabled: enabled != 0,
                    mode,
                    host,
                    port: port as u16,
                    from_address,
                    from_name,
                    username,
                    password,
                    has_password,
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

        let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM smtp_config WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        if exists.0 > 0 {
            if password_encrypted.is_some() {
                sqlx::query(
                    "UPDATE smtp_config SET enabled = ?, mode = ?, host = ?, port = ?, from_address = ?, from_name = ?, username = ?, password_encrypted = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
                )
                .bind(if config.enabled { 1 } else { 0 })
                .bind(config.mode.to_string())
                .bind(&config.host)
                .bind(config.port as i32)
                .bind(&config.from_address)
                .bind(&config.from_name)
                .bind(&config.username)
                .bind(&password_encrypted)
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to update config: {}", e))?;
            } else {
                sqlx::query(
                    "UPDATE smtp_config SET enabled = ?, mode = ?, host = ?, port = ?, from_address = ?, from_name = ?, username = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
                )
                .bind(if config.enabled { 1 } else { 0 })
                .bind(config.mode.to_string())
                .bind(&config.host)
                .bind(config.port as i32)
                .bind(&config.from_address)
                .bind(&config.from_name)
                .bind(&config.username)
                .execute(&self.pool)
                .await
                .map_err(|e| format!("Failed to update config: {}", e))?;
            }
        } else {
            sqlx::query(
                "INSERT INTO smtp_config (id, enabled, mode, host, port, from_address, from_name, username, password_encrypted) VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(if config.enabled { 1 } else { 0 })
            .bind(config.mode.to_string())
            .bind(&config.host)
            .bind(config.port as i32)
            .bind(&config.from_address)
            .bind(&config.from_name)
            .bind(&config.username)
            .bind(&password_encrypted)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("Failed to insert config: {}", e))?;
        }

        debug!("SMTP configuration saved");
        Ok(())
    }
}
