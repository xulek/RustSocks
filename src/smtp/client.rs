use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use tracing::info;

use super::types::{SmtpConfig, SmtpMode};

pub struct SmtpClient {
    config: SmtpConfig,
}

impl SmtpClient {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    fn build_transport(&self) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
        let host = &self.config.host;
        let port = self.config.port;

        let builder = match self.config.mode {
            SmtpMode::PlainNoauth | SmtpMode::PlainAuth => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host).port(port)
            }
            SmtpMode::StarttlsNoauth | SmtpMode::StarttlsAuth | SmtpMode::StarttlsRequired => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                    .map_err(|e| format!("Failed to create STARTTLS transport: {}", e))?
                    .port(port)
            }
            SmtpMode::SmtpsNoauth | SmtpMode::SmtpsAuth => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                    .map_err(|e| format!("Failed to create SMTPS transport: {}", e))?
                    .port(port)
            }
        };

        let transport = if self.config.mode.requires_auth() {
            let username = self
                .config
                .username
                .as_ref()
                .ok_or("Username required for authentication")?;
            let password = self
                .config
                .password
                .as_ref()
                .ok_or("Password required for authentication")?;

            let credentials = Credentials::new(username.clone(), password.clone());
            builder.credentials(credentials).build()
        } else {
            builder.build()
        };

        Ok(transport)
    }

    pub async fn send_test(&self, recipient: &str) -> Result<(), String> {
        self.send(
            recipient,
            "RustSocks SMTP Test",
            "This is a test email from RustSocks.\n\nIf you received this message, your SMTP configuration is working correctly.",
        )
        .await
    }

    pub async fn send(&self, recipient: &str, subject: &str, body: &str) -> Result<(), String> {
        if !self.config.enabled {
            return Err("SMTP is not enabled".to_string());
        }

        if self.config.host.is_empty() {
            return Err("SMTP host is not configured".to_string());
        }

        let from_mailbox: Mailbox = if let Some(ref name) = self.config.from_name {
            format!("{} <{}>", name, self.config.from_address)
                .parse()
                .map_err(|e| format!("Invalid from address: {}", e))?
        } else {
            self.config
                .from_address
                .parse()
                .map_err(|e| format!("Invalid from address: {}", e))?
        };

        let to_mailbox: Mailbox = recipient
            .parse()
            .map_err(|e| format!("Invalid recipient address: {}", e))?;

        let email = Message::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .body(body.to_string())
            .map_err(|e| format!("Failed to build email: {}", e))?;

        let transport = self.build_transport()?;

        transport
            .send(email)
            .await
            .map_err(|e| format!("Failed to send email: {}", e))?;

        info!("Email sent successfully to {}", recipient);
        Ok(())
    }
}
