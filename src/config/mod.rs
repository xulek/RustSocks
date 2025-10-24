use crate::utils::error::{Result, RustSocksError};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub acl: AclSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_bind_port")]
    pub bind_port: u16,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_method")]
    pub method: String, // "none", "userpass"
    #[serde(default)]
    pub users: Vec<User>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String, // "json" or "pretty"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclSettings {
    #[serde(default = "default_acl_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub config_file: Option<String>,
    #[serde(default = "default_acl_watch")]
    pub watch: bool,
    #[serde(default = "default_acl_anonymous_user")]
    pub anonymous_user: String,
}

// Default values
fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_bind_port() -> u16 {
    1080
}

fn default_max_connections() -> usize {
    1000
}

fn default_auth_method() -> String {
    "none".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_acl_enabled() -> bool {
    false
}

fn default_acl_watch() -> bool {
    false
}

fn default_acl_anonymous_user() -> String {
    "anonymous".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            bind_port: default_bind_port(),
            max_connections: default_max_connections(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            method: default_auth_method(),
            users: Vec::new(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

impl Default for AclSettings {
    fn default() -> Self {
        Self {
            enabled: default_acl_enabled(),
            config_file: None,
            watch: default_acl_watch(),
            anonymous_user: default_acl_anonymous_user(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            logging: LoggingConfig::default(),
            acl: AclSettings::default(),
        }
    }
}

impl Config {
    /// Load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| RustSocksError::Config(format!("Failed to read config file: {}", e)))?;

        let config: Config = toml::from_str(&content)
            .map_err(|e| RustSocksError::Config(format!("Failed to parse config: {}", e)))?;

        config.validate()?;

        Ok(config)
    }

    /// Validate configuration
    fn validate(&self) -> Result<()> {
        // Validate auth method
        if !matches!(self.auth.method.as_str(), "none" | "userpass") {
            return Err(RustSocksError::Config(format!(
                "Invalid auth method: {}. Must be 'none' or 'userpass'",
                self.auth.method
            )));
        }

        // If userpass auth, ensure we have users
        if self.auth.method == "userpass" && self.auth.users.is_empty() {
            return Err(RustSocksError::Config(
                "userpass auth requires at least one user".to_string(),
            ));
        }

        if self.acl.enabled {
            let path = self.acl.config_file.as_ref().ok_or_else(|| {
                RustSocksError::Config("ACL enabled but no config_file provided".to_string())
            })?;

            if path.trim().is_empty() {
                return Err(RustSocksError::Config(
                    "ACL config_file cannot be empty when ACL is enabled".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Create example configuration file
    pub fn create_example<P: AsRef<Path>>(path: P) -> Result<()> {
        let example = r#"[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 1000

[auth]
method = "none"  # Options: "none", "userpass"

# For userpass authentication, add users:
# [[auth.users]]
# username = "alice"
# password = "secret123"

[logging]
level = "info"  # Options: "trace", "debug", "info", "warn", "error"
format = "pretty"  # Options: "pretty", "json"

[acl]
enabled = false
config_file = "config/acl.toml"
watch = false
anonymous_user = "anonymous"
"#;

        std::fs::write(path.as_ref(), example).map_err(|e| {
            RustSocksError::Config(format!("Failed to write example config: {}", e))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.bind_address, "127.0.0.1");
        assert_eq!(config.server.bind_port, 1080);
        assert_eq!(config.auth.method, "none");
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        config.auth.method = "invalid".to_string();
        assert!(config.validate().is_err());

        config.auth.method = "userpass".to_string();
        assert!(config.validate().is_err()); // No users

        config.auth.users.push(User {
            username: "test".to_string(),
            password: "pass".to_string(),
        });
        assert!(config.validate().is_ok());

        // ACL enabled without file should fail
        let mut config = Config::default();
        config.acl.enabled = true;
        assert!(config.validate().is_err());

        // ACL enabled with file works
        config.acl.config_file = Some("config/acl.toml".to_string());
        assert!(config.validate().is_ok());
    }
}
