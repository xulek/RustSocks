use crate::utils::error::{Result, RustSocksError};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub acl: AclSettings,
    #[serde(default)]
    pub sessions: SessionSettings,
    #[serde(default)]
    pub metrics: MetricsSettings,
    #[serde(default)]
    pub qos: crate::qos::QosConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_bind_port")]
    pub bind_port: u16,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default)]
    pub tls: TlsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSettings {
    #[serde(default = "default_tls_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub certificate_path: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub key_password: Option<String>,
    #[serde(default = "default_tls_require_client_auth")]
    pub require_client_auth: bool,
    #[serde(default)]
    pub client_ca_path: Option<String>,
    #[serde(default)]
    pub alpn_protocols: Vec<String>,
    #[serde(default)]
    pub min_protocol_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_client_method")]
    pub client_method: String, // "none", "pam.address"
    #[serde(default = "default_socks_method", alias = "method")]
    pub socks_method: String, // "none", "userpass", "pam.address", "pam.username"
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub pam: PamSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PamSettings {
    #[serde(default = "default_pam_username_service")]
    pub username_service: String,
    #[serde(default = "default_pam_address_service")]
    pub address_service: String,
    #[serde(default = "default_pam_default_user")]
    pub default_user: String,
    #[serde(default = "default_pam_default_ruser")]
    pub default_ruser: String,
    #[serde(default)]
    pub verbose: bool,
    #[serde(default)]
    pub verify_service: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSettings {
    #[serde(default = "default_sessions_enabled")]
    pub enabled: bool,
    #[serde(default = "default_session_storage")]
    pub storage: String,
    #[serde(default)]
    pub database_url: Option<String>,
    #[serde(default = "default_session_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_session_batch_interval_ms")]
    pub batch_interval_ms: u64,
    #[serde(default = "default_session_retention_days")]
    pub retention_days: u64,
    #[serde(default = "default_session_cleanup_interval_hours")]
    pub cleanup_interval_hours: u64,
    #[serde(default = "default_session_traffic_update_packet_interval")]
    pub traffic_update_packet_interval: u64,
    #[serde(default = "default_stats_window_hours")]
    pub stats_window_hours: u64,
    #[serde(default = "default_stats_api_enabled")]
    pub stats_api_enabled: bool,
    #[serde(default = "default_stats_api_bind_address")]
    pub stats_api_bind_address: String,
    #[serde(default = "default_stats_api_port")]
    pub stats_api_port: u16,
    #[serde(default = "default_swagger_enabled")]
    pub swagger_enabled: bool,
    #[serde(default = "default_dashboard_enabled")]
    pub dashboard_enabled: bool,
    #[serde(default = "default_base_path")]
    pub base_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSettings {
    #[serde(default = "default_metrics_enabled")]
    pub enabled: bool,
    #[serde(default = "default_metrics_storage")]
    pub storage: String, // "memory" or "sqlite"
    #[serde(default = "default_metrics_retention_hours")]
    pub retention_hours: u64,
    #[serde(default = "default_metrics_cleanup_interval_hours")]
    pub cleanup_interval_hours: u64,
    #[serde(default = "default_metrics_collection_interval_secs")]
    pub collection_interval_secs: u64,
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

fn default_tls_enabled() -> bool {
    false
}

fn default_tls_require_client_auth() -> bool {
    false
}

fn default_client_method() -> String {
    "none".to_string()
}

fn default_socks_method() -> String {
    "none".to_string()
}

fn default_pam_username_service() -> String {
    "rustsocks".to_string()
}

fn default_pam_address_service() -> String {
    "rustsocks-client".to_string()
}

fn default_pam_default_user() -> String {
    "rhostusr".to_string()
}

fn default_pam_default_ruser() -> String {
    "rhostusr".to_string()
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

fn default_sessions_enabled() -> bool {
    false
}

fn default_session_storage() -> String {
    "memory".to_string()
}

fn default_session_batch_size() -> usize {
    100
}

fn default_session_batch_interval_ms() -> u64 {
    1000
}

fn default_session_retention_days() -> u64 {
    90
}

fn default_session_cleanup_interval_hours() -> u64 {
    24
}

fn default_session_traffic_update_packet_interval() -> u64 {
    10
}

fn default_stats_window_hours() -> u64 {
    24
}

fn default_stats_api_enabled() -> bool {
    false
}

fn default_stats_api_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_stats_api_port() -> u16 {
    9090
}

fn default_swagger_enabled() -> bool {
    true
}

fn default_dashboard_enabled() -> bool {
    false
}

fn default_base_path() -> String {
    "/".to_string()
}

fn default_metrics_enabled() -> bool {
    true
}

fn default_metrics_storage() -> String {
    "memory".to_string()
}

fn default_metrics_retention_hours() -> u64 {
    24
}

fn default_metrics_cleanup_interval_hours() -> u64 {
    6
}

fn default_metrics_collection_interval_secs() -> u64 {
    5
}

fn normalize_base_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }

    let mut normalized = String::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() {
            continue;
        }
        normalized.push('/');
        normalized.push_str(segment);
    }

    if normalized.is_empty() {
        "/".to_string()
    } else {
        normalized
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            bind_port: default_bind_port(),
            max_connections: default_max_connections(),
            tls: TlsSettings::default(),
        }
    }
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            enabled: default_tls_enabled(),
            certificate_path: None,
            private_key_path: None,
            key_password: None,
            require_client_auth: default_tls_require_client_auth(),
            client_ca_path: None,
            alpn_protocols: Vec::new(),
            min_protocol_version: None,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            client_method: default_client_method(),
            socks_method: default_socks_method(),
            users: Vec::new(),
            pam: PamSettings::default(),
        }
    }
}

impl Default for PamSettings {
    fn default() -> Self {
        Self {
            username_service: default_pam_username_service(),
            address_service: default_pam_address_service(),
            default_user: default_pam_default_user(),
            default_ruser: default_pam_default_ruser(),
            verbose: false,
            verify_service: false,
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

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            enabled: default_sessions_enabled(),
            storage: default_session_storage(),
            database_url: None,
            batch_size: default_session_batch_size(),
            batch_interval_ms: default_session_batch_interval_ms(),
            retention_days: default_session_retention_days(),
            cleanup_interval_hours: default_session_cleanup_interval_hours(),
            traffic_update_packet_interval: default_session_traffic_update_packet_interval(),
            stats_window_hours: default_stats_window_hours(),
            stats_api_enabled: default_stats_api_enabled(),
            stats_api_bind_address: default_stats_api_bind_address(),
            stats_api_port: default_stats_api_port(),
            swagger_enabled: default_swagger_enabled(),
            dashboard_enabled: default_dashboard_enabled(),
            base_path: default_base_path(),
        }
    }
}

impl Default for MetricsSettings {
    fn default() -> Self {
        Self {
            enabled: default_metrics_enabled(),
            storage: default_metrics_storage(),
            retention_hours: default_metrics_retention_hours(),
            cleanup_interval_hours: default_metrics_cleanup_interval_hours(),
            collection_interval_secs: default_metrics_collection_interval_secs(),
        }
    }
}

impl Config {
    /// Load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| RustSocksError::Config(format!("Failed to read config file: {}", e)))?;

        let mut config: Config = toml::from_str(&content)
            .map_err(|e| RustSocksError::Config(format!("Failed to parse config: {}", e)))?;

        config.validate()?;

        // Normalize base path after validation so downstream components can rely on canonical form.
        let normalized_base = config.sessions.normalized_base_path();
        config.sessions.base_path = normalized_base;

        Ok(config)
    }

    /// Validate configuration
    fn validate(&self) -> Result<()> {
        // Validate authentication configuration
        if !matches!(self.auth.client_method.as_str(), "none" | "pam.address") {
            return Err(RustSocksError::Config(format!(
                "Invalid client auth method: {}. Supported: none, pam.address",
                self.auth.client_method
            )));
        }

        if !matches!(
            self.auth.socks_method.as_str(),
            "none" | "userpass" | "pam.address" | "pam.username"
        ) {
            return Err(RustSocksError::Config(format!(
                "Invalid SOCKS auth method: {}. Supported: none, userpass, pam.address, pam.username",
                self.auth.socks_method
            )));
        }

        #[cfg(not(unix))]
        {
            if self.auth.client_method == "pam.address"
                || matches!(
                    self.auth.socks_method.as_str(),
                    "pam.address" | "pam.username"
                )
            {
                return Err(RustSocksError::Config(
                    "PAM authentication is only supported on Unix-like systems".to_string(),
                ));
            }
        }

        if self.auth.socks_method == "userpass" && self.auth.users.is_empty() {
            return Err(RustSocksError::Config(
                "userpass auth requires at least one user".to_string(),
            ));
        }

        if self.auth.socks_method == "pam.username"
            && self.auth.pam.username_service.trim().is_empty()
        {
            return Err(RustSocksError::Config(
                "auth.pam.username_service cannot be empty when pam.username auth is enabled"
                    .to_string(),
            ));
        }

        if (self.auth.socks_method == "pam.address" || self.auth.client_method == "pam.address")
            && self.auth.pam.address_service.trim().is_empty()
        {
            return Err(RustSocksError::Config(
                "auth.pam.address_service cannot be empty when pam.address auth is enabled"
                    .to_string(),
            ));
        }

        if self.server.tls.enabled {
            let cert_path = self.server.tls.certificate_path.as_ref().ok_or_else(|| {
                RustSocksError::Config(
                    "server.tls.enabled is true but certificate_path is not set".to_string(),
                )
            })?;
            if cert_path.trim().is_empty() {
                return Err(RustSocksError::Config(
                    "server.tls.certificate_path cannot be empty when TLS is enabled".to_string(),
                ));
            }

            let key_path = self.server.tls.private_key_path.as_ref().ok_or_else(|| {
                RustSocksError::Config(
                    "server.tls.enabled is true but private_key_path is not set".to_string(),
                )
            })?;
            if key_path.trim().is_empty() {
                return Err(RustSocksError::Config(
                    "server.tls.private_key_path cannot be empty when TLS is enabled".to_string(),
                ));
            }

            if self.server.tls.require_client_auth
                && self
                    .server
                    .tls
                    .client_ca_path
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            {
                return Err(RustSocksError::Config(
                    "server.tls.client_ca_path is required when require_client_auth is true"
                        .to_string(),
                ));
            }

            if let Some(min_ver) = self.server.tls.min_protocol_version.as_deref() {
                if !matches!(min_ver, "TLS12" | "TLS13") {
                    return Err(RustSocksError::Config(format!(
                        "Invalid server.tls.min_protocol_version '{}'. Supported values: TLS12, TLS13",
                        min_ver
                    )));
                }
            }
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

        if !matches!(self.sessions.storage.as_str(), "memory" | "sqlite") {
            return Err(RustSocksError::Config(format!(
                "Invalid session storage: {}. Supported: memory, sqlite",
                self.sessions.storage
            )));
        }

        if self.sessions.enabled
            && self.sessions.storage == "sqlite"
            && self.sessions.database_url.is_none()
        {
            return Err(RustSocksError::Config(
                "sessions.database_url is required when session tracking uses sqlite storage"
                    .to_string(),
            ));
        }

        if self.sessions.cleanup_interval_hours == 0 {
            return Err(RustSocksError::Config(
                "sessions.cleanup_interval_hours must be greater than 0".to_string(),
            ));
        }

        if self.sessions.traffic_update_packet_interval == 0 {
            return Err(RustSocksError::Config(
                "sessions.traffic_update_packet_interval must be greater than 0".to_string(),
            ));
        }

        if self.sessions.stats_window_hours == 0 {
            return Err(RustSocksError::Config(
                "sessions.stats_window_hours must be greater than 0".to_string(),
            ));
        }

        if self.sessions.base_path.trim().is_empty() {
            return Err(RustSocksError::Config(
                "sessions.base_path cannot be empty".to_string(),
            ));
        }

        if self.sessions.base_path.chars().any(|c| c.is_whitespace()) {
            return Err(RustSocksError::Config(
                "sessions.base_path cannot contain whitespace".to_string(),
            ));
        }

        let normalized_base = self.sessions.normalized_base_path();
        if !normalized_base.starts_with('/') {
            return Err(RustSocksError::Config(
                "sessions.base_path must resolve to an absolute path starting with '/'".to_string(),
            ));
        }

        // Validate metrics configuration
        if !matches!(self.metrics.storage.as_str(), "memory" | "sqlite") {
            return Err(RustSocksError::Config(format!(
                "Invalid metrics storage: {}. Supported: memory, sqlite",
                self.metrics.storage
            )));
        }

        if self.metrics.cleanup_interval_hours == 0 {
            return Err(RustSocksError::Config(
                "metrics.cleanup_interval_hours must be greater than 0".to_string(),
            ));
        }

        if self.metrics.collection_interval_secs == 0 {
            return Err(RustSocksError::Config(
                "metrics.collection_interval_secs must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Create example configuration file
    pub fn create_example<P: AsRef<Path>>(path: P) -> Result<()> {
        let example = r#"[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 1000

[server.tls]
enabled = false
certificate_path = "config/server.crt"
private_key_path = "config/server.key"
require_client_auth = false
# client_ca_path = "config/ca.crt"
# alpn_protocols = ["socks"]
# min_protocol_version = "TLS13"

[auth]
client_method = "none"       # Options: "none", "pam.address"
socks_method = "none"        # Options: "none", "userpass", "pam.address", "pam.username"

# For userpass authentication, add users:
# [[auth.users]]
# username = "alice"
# password = "secret123"

[auth.pam]
# PAM service names (Linux /etc/pam.d/<service>)
username_service = "rustsocks"
address_service = "rustsocks-client"

# Default identity for pam.address when username is not provided
default_user = "rhostusr"
default_ruser = "rhostusr"

# Set to true to enable verbose PAM logging and service validation
verbose = false
verify_service = false

[logging]
level = "info"  # Options: "trace", "debug", "info", "warn", "error"
format = "pretty"  # Options: "pretty", "json"

[acl]
enabled = false
config_file = "config/acl.toml"
watch = false
anonymous_user = "anonymous"

[sessions]
enabled = false
storage = "memory"  # Options: "memory", "sqlite"
# database_url = "sqlite://var/lib/rustsocks/sessions.db"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24
traffic_update_packet_interval = 10
stats_window_hours = 24
stats_api_enabled = false
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
swagger_enabled = true
dashboard_enabled = false
base_path = "/"

[metrics]
enabled = true              # Enable metrics collection
storage = "memory"          # Options: "memory", "sqlite" (uses sessions.database_url)
retention_hours = 24        # Keep metrics for 24 hours
cleanup_interval_hours = 6  # Cleanup old metrics every 6 hours
collection_interval_secs = 5  # Collect metrics every 5 seconds

[qos]
enabled = false  # Enable QoS (Quality of Service) / Rate Limiting
algorithm = "htb"  # Options: "htb" (Hierarchical Token Bucket with fair sharing)

[qos.htb]
# Global bandwidth limit (1 Gbps = 125 MB/s = 125000000 bytes/sec)
global_bandwidth_bytes_per_sec = 125000000

# Per-user guaranteed minimum bandwidth (1 Mbps = 131072 bytes/sec)
guaranteed_bandwidth_bytes_per_sec = 131072

# Per-user maximum bandwidth when borrowing (100 Mbps = 12500000 bytes/sec)
max_bandwidth_bytes_per_sec = 12500000

# Burst size (how much can be transferred instantly)
burst_size_bytes = 1048576  # 1 MB

# Token bucket refill interval (milliseconds)
refill_interval_ms = 50

# Fair sharing - dynamically allocate unused bandwidth to active users
fair_sharing_enabled = true

# How often to recalculate fair shares (milliseconds)
rebalance_interval_ms = 100

# User inactivity timeout (seconds) - user considered idle after this period
idle_timeout_secs = 5

[qos.connection_limits]
# Maximum connections per user
max_connections_per_user = 20

# Maximum total connections (global)
max_connections_global = 10000
"#;

        std::fs::write(path.as_ref(), example).map_err(|e| {
            RustSocksError::Config(format!("Failed to write example config: {}", e))
        })?;

        Ok(())
    }
}

impl SessionSettings {
    pub fn normalized_base_path(&self) -> String {
        normalize_base_path(&self.base_path)
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
        assert_eq!(config.auth.client_method, "none");
        assert_eq!(config.auth.socks_method, "none");
        assert_eq!(config.sessions.storage, "memory");
        assert_eq!(config.sessions.batch_size, 100);
        assert_eq!(config.sessions.batch_interval_ms, 1000);
        assert_eq!(config.sessions.retention_days, 90);
        assert_eq!(config.sessions.cleanup_interval_hours, 24);
        assert_eq!(config.sessions.storage, "memory");
        assert_eq!(config.sessions.batch_size, 100);
        assert_eq!(config.sessions.traffic_update_packet_interval, 10);
        assert_eq!(config.sessions.stats_window_hours, 24);
        assert!(!config.sessions.stats_api_enabled);
        assert_eq!(config.sessions.stats_api_bind_address, "127.0.0.1");
        assert_eq!(config.sessions.stats_api_port, 9090);
        assert!(config.sessions.swagger_enabled);
        assert!(!config.sessions.dashboard_enabled);
        assert_eq!(config.sessions.base_path, "/");
        assert_eq!(config.sessions.normalized_base_path(), "/");
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        config.auth.socks_method = "invalid".to_string();
        assert!(config.validate().is_err());

        config.auth.socks_method = "userpass".to_string();
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

        // Invalid session storage
        let mut config = Config::default();
        config.sessions.storage = "invalid".to_string();
        assert!(config.validate().is_err());

        // Missing database_url when sqlite enabled
        let mut config = Config::default();
        config.sessions.enabled = true;
        config.sessions.storage = "sqlite".to_string();
        assert!(config.validate().is_err());

        config.sessions.database_url = Some("sqlite::memory:".to_string());
        assert!(config.validate().is_ok());

        config.sessions.cleanup_interval_hours = 0;
        assert!(config.validate().is_err());

        config.sessions.cleanup_interval_hours = 12;
        assert!(config.validate().is_ok());

        config.sessions.stats_window_hours = 0;
        assert!(config.validate().is_err());

        let mut config = Config::default();
        config.sessions.base_path = "".to_string();
        assert!(config.validate().is_err());

        config.sessions.base_path = " rust".to_string();
        assert!(config.validate().is_err());

        config.sessions.base_path = "/rustsocks/".to_string();
        assert!(config.validate().is_ok());
        assert_eq!(config.sessions.normalized_base_path(), "/rustsocks");
    }
}
