use std::sync::Arc;

use tracing::debug;

use super::types::SmtpConfig;

pub struct SmtpRepository {
    store: Arc<crate::session::SessionStore>,
    api_token: Option<String>,
}

impl SmtpRepository {
    pub fn new(store: Arc<crate::session::SessionStore>, api_token: Option<String>) -> Self {
        Self { store, api_token }
    }

    pub async fn get_config(&self) -> Result<SmtpConfig, String> {
        self.store.load_smtp_config(self.api_token.as_deref()).await
    }

    pub async fn save_config(
        &self,
        config: &SmtpConfig,
        new_password: Option<&str>,
    ) -> Result<(), String> {
        self.store
            .save_smtp_config(config, new_password, self.api_token.as_deref())
            .await?;
        debug!("SMTP configuration saved");
        Ok(())
    }
}

#[cfg(all(test, feature = "database"))]
mod tests {
    use super::*;
    use crate::smtp::{SmtpConfig, SmtpMode};

    #[tokio::test]
    async fn save_and_load_config_roundtrip() {
        let store = Arc::new(
            crate::session::SessionStore::connect("sqlite::memory:")
                .await
                .expect("store"),
        );
        let repo = SmtpRepository::new(store, Some("secret-token".to_string()));

        let config = SmtpConfig {
            enabled: true,
            mode: SmtpMode::StarttlsRequired,
            host: "smtp.example.com".to_string(),
            port: 2525,
            from_address: "noreply@example.com".to_string(),
            from_name: Some("RustSocks".to_string()),
            username: Some("mailer".to_string()),
            password: None,
            has_password: false,
            notify_recipients: vec!["ops@example.com".to_string()],
            notify_critical: true,
            notify_security: true,
            notify_config_changes: false,
            notify_service_status: true,
            notify_resource_pressure: false,
            notify_connection_pressure: true,
            notify_cooldown_seconds: 30,
            notify_cpu_threshold: 80,
            notify_ram_threshold: 81,
            notify_disk_threshold: 82,
            notify_connection_percent_threshold: 83,
        };

        repo.save_config(&config, Some("topsecret"))
            .await
            .expect("save config");

        let loaded = repo.get_config().await.expect("load config");
        assert!(loaded.enabled);
        assert_eq!(loaded.mode, SmtpMode::StarttlsRequired);
        assert_eq!(loaded.host, "smtp.example.com");
        assert_eq!(loaded.port, 2525);
        assert_eq!(loaded.from_name.as_deref(), Some("RustSocks"));
        assert_eq!(loaded.username.as_deref(), Some("mailer"));
        assert!(loaded.has_password);
        assert_eq!(
            loaded.notify_recipients,
            vec!["ops@example.com".to_string()]
        );
        assert!(loaded.notify_security);
        assert!(loaded.notify_connection_pressure);
    }
}
