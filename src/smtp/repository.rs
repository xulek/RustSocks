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
