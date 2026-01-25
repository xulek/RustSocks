use serde::{Deserialize, Serialize};

/// SMTP connection modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SmtpMode {
    PlainNoauth,
    PlainAuth,
    StarttlsNoauth,
    #[default]
    StarttlsAuth,
    StarttlsRequired,
    SmtpsNoauth,
    SmtpsAuth,
}

impl SmtpMode {
    pub fn default_port(&self) -> u16 {
        match self {
            SmtpMode::PlainNoauth | SmtpMode::PlainAuth => 1025,
            SmtpMode::StarttlsNoauth | SmtpMode::StarttlsAuth | SmtpMode::StarttlsRequired => 587,
            SmtpMode::SmtpsNoauth | SmtpMode::SmtpsAuth => 465,
        }
    }

    pub fn requires_auth(&self) -> bool {
        matches!(
            self,
            SmtpMode::PlainAuth
                | SmtpMode::StarttlsAuth
                | SmtpMode::StarttlsRequired
                | SmtpMode::SmtpsAuth
        )
    }

    pub fn uses_tls(&self) -> bool {
        !matches!(self, SmtpMode::PlainNoauth | SmtpMode::PlainAuth)
    }

    pub fn uses_starttls(&self) -> bool {
        matches!(
            self,
            SmtpMode::StarttlsNoauth | SmtpMode::StarttlsAuth | SmtpMode::StarttlsRequired
        )
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            SmtpMode::PlainNoauth => "Plain (no auth)",
            SmtpMode::PlainAuth => "Plain with auth",
            SmtpMode::StarttlsNoauth => "STARTTLS (no auth)",
            SmtpMode::StarttlsAuth => "STARTTLS with auth",
            SmtpMode::StarttlsRequired => "STARTTLS required",
            SmtpMode::SmtpsNoauth => "SMTPS/SSL (no auth)",
            SmtpMode::SmtpsAuth => "SMTPS/SSL with auth",
        }
    }
}

impl std::fmt::Display for SmtpMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SmtpMode::PlainNoauth => "plain_noauth",
            SmtpMode::PlainAuth => "plain_auth",
            SmtpMode::StarttlsNoauth => "starttls_noauth",
            SmtpMode::StarttlsAuth => "starttls_auth",
            SmtpMode::StarttlsRequired => "starttls_required",
            SmtpMode::SmtpsNoauth => "smtps_noauth",
            SmtpMode::SmtpsAuth => "smtps_auth",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for SmtpMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "plain_noauth" => Ok(SmtpMode::PlainNoauth),
            "plain_auth" => Ok(SmtpMode::PlainAuth),
            "starttls_noauth" => Ok(SmtpMode::StarttlsNoauth),
            "starttls_auth" => Ok(SmtpMode::StarttlsAuth),
            "starttls_required" => Ok(SmtpMode::StarttlsRequired),
            "smtps_noauth" => Ok(SmtpMode::SmtpsNoauth),
            "smtps_auth" => Ok(SmtpMode::SmtpsAuth),
            _ => Err(format!("Unknown SMTP mode: {}", s)),
        }
    }
}

/// SMTP configuration stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub enabled: bool,
    pub mode: SmtpMode,
    pub host: String,
    pub port: u16,
    pub from_address: String,
    pub from_name: Option<String>,
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    #[serde(skip_deserializing)]
    pub has_password: bool,
    pub notify_recipients: Vec<String>,
    pub notify_critical: bool,
    pub notify_security: bool,
    pub notify_config_changes: bool,
    pub notify_service_status: bool,
    pub notify_resource_pressure: bool,
    pub notify_connection_pressure: bool,
    pub notify_cooldown_seconds: u64,
    pub notify_cpu_threshold: u8,
    pub notify_ram_threshold: u8,
    pub notify_disk_threshold: u8,
    pub notify_connection_percent_threshold: u8,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: SmtpMode::default(),
            host: String::new(),
            port: 587,
            from_address: String::new(),
            from_name: None,
            username: None,
            password: None,
            has_password: false,
            notify_recipients: Vec::new(),
            notify_critical: false,
            notify_security: false,
            notify_config_changes: false,
            notify_service_status: false,
            notify_resource_pressure: false,
            notify_connection_pressure: false,
            notify_cooldown_seconds: 3600,
            notify_cpu_threshold: 85,
            notify_ram_threshold: 85,
            notify_disk_threshold: 90,
            notify_connection_percent_threshold: 85,
        }
    }
}

pub fn parse_recipients(raw: &str) -> Vec<String> {
    raw.split([',', '\n'])
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

pub fn format_recipients(recipients: &[String]) -> String {
    recipients.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_mode_roundtrip() {
        let mode: SmtpMode = "starttls_required".parse().unwrap();
        assert_eq!(mode, SmtpMode::StarttlsRequired);
        assert_eq!(mode.to_string(), "starttls_required");
    }

    #[test]
    fn smtp_mode_helpers() {
        let mode = SmtpMode::SmtpsAuth;
        assert_eq!(mode.default_port(), 465);
        assert!(mode.requires_auth());
        assert!(mode.uses_tls());
        assert!(!mode.uses_starttls());
    }

    #[test]
    fn recipients_parse_and_format() {
        let raw = "alpha@example.com, beta@example.com\n\ngamma@example.com , ";
        let parsed = parse_recipients(raw);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], "alpha@example.com");
        assert_eq!(parsed[1], "beta@example.com");
        assert_eq!(parsed[2], "gamma@example.com");

        let formatted = format_recipients(&parsed);
        assert_eq!(formatted, "alpha@example.com, beta@example.com, gamma@example.com");
    }
}
