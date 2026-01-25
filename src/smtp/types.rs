use serde::{Deserialize, Serialize};

/// SMTP connection modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpMode {
    PlainNoauth,
    PlainAuth,
    StarttlsNoauth,
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

impl Default for SmtpMode {
    fn default() -> Self {
        SmtpMode::StarttlsAuth
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
        }
    }
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
}
