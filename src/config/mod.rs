use crate::utils::error::{Result, RustSocksError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

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
    pub telemetry: TelemetrySettings,
    #[serde(default)]
    pub qos: crate::qos::QosConfig,
    #[serde(default)]
    pub policy_state: PolicyStateSettings,
}

/// Where dynamic-policy usage counters (active connections, connection rate and
/// transfer quotas) are kept.
///
/// `memory` keeps them in the process: simple, but quotas reset on restart and each
/// instance enforces its own limits. `redis` shares them between instances so limits
/// apply to the whole deployment (requires building with the `redis` feature).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStateSettings {
    /// `memory` or `redis`.
    #[serde(default = "default_policy_state_backend")]
    pub backend: String,
    /// Redis connection URL (supports `${ENV}` expansion), e.g. `redis://127.0.0.1:6379/0`.
    #[serde(default)]
    pub redis_url: Option<String>,
    /// Prefix for every Redis key, so several deployments can share one Redis.
    #[serde(default = "default_policy_state_key_prefix")]
    pub key_prefix: String,
    /// Behaviour when Redis is unreachable for a policy that has limits:
    /// `fail_closed` denies the connection, `fail_open` falls back to this
    /// instance's local counters.
    #[serde(default = "default_policy_state_failure_mode")]
    pub failure_mode: String,
    /// Lifetime of an active-connection lease. A crashed instance's connections stop
    /// counting once their lease expires; live instances renew it periodically.
    #[serde(default = "default_policy_state_lease_secs")]
    pub lease_secs: u64,
    /// Upper bound for one Redis operation on the connection path.
    #[serde(default = "default_policy_state_operation_timeout_ms")]
    pub operation_timeout_ms: u64,
}

fn default_policy_state_backend() -> String {
    "memory".to_string()
}

fn default_policy_state_key_prefix() -> String {
    "rustsocks".to_string()
}

fn default_policy_state_failure_mode() -> String {
    "fail_closed".to_string()
}

fn default_policy_state_lease_secs() -> u64 {
    60
}

fn default_policy_state_operation_timeout_ms() -> u64 {
    250
}

impl Default for PolicyStateSettings {
    fn default() -> Self {
        Self {
            backend: default_policy_state_backend(),
            redis_url: None,
            key_prefix: default_policy_state_key_prefix(),
            failure_mode: default_policy_state_failure_mode(),
            lease_secs: default_policy_state_lease_secs(),
            operation_timeout_ms: default_policy_state_operation_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_bind_port")]
    pub bind_port: u16,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// Maximum concurrent connections from one client address (IPv6 clients are
    /// grouped by /64). `0` disables the per-address limit.
    #[serde(default)]
    pub max_connections_per_ip: usize,
    #[serde(default = "default_handshake_timeout_ms")]
    pub handshake_timeout_ms: u64,
    #[serde(default)]
    pub allow_unsafe_public_proxy: bool,
    #[serde(default)]
    pub tls: TlsSettings,
    #[serde(default)]
    pub pool: PoolSettings,
    #[serde(default)]
    pub resolver: ResolverSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolverSettings {
    #[serde(default = "default_dns_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_dns_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
    #[serde(default = "default_dns_cache_max_entries")]
    pub cache_max_entries: usize,
    #[serde(default = "default_dns_max_concurrent_lookups")]
    pub max_concurrent_lookups: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_pool_max_idle_per_dest")]
    pub max_idle_per_dest: usize,
    #[serde(default = "default_pool_max_total_idle")]
    pub max_total_idle: usize,
    #[serde(default = "default_pool_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_pool_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
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
    #[serde(default = "default_tls_handshake_timeout_ms")]
    pub handshake_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_client_method")]
    pub client_method: String, // "none", "pam.address"
    #[serde(default = "default_socks_method", alias = "method")]
    pub socks_method: String, // "none", "userpass", "pam.address", "pam.username", "gssapi"
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub pam: PamSettings,
    #[serde(default)]
    pub gssapi: GssApiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardRoleAssignment {
    pub username: String,
    pub role: String,
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
pub struct GssApiSettings {
    #[serde(default = "default_gssapi_service_name")]
    pub service_name: String,
    #[serde(default)]
    pub keytab_path: Option<String>,
    #[serde(default = "default_gssapi_protection_level")]
    pub protection_level: String,
    #[serde(default)]
    pub verbose: bool,
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
    #[serde(default = "default_session_batch_queue_capacity")]
    pub batch_queue_capacity: usize,
    #[serde(default = "default_session_history_max_entries")]
    pub history_max_entries: usize,
    #[serde(default = "default_session_retention_days")]
    pub retention_days: u64,
    #[serde(default = "default_session_cleanup_interval_hours")]
    pub cleanup_interval_hours: u64,
    #[serde(default = "default_session_traffic_update_packet_interval")]
    pub traffic_update_packet_interval: u64,
    #[serde(default = "default_session_traffic_queue_capacity")]
    pub traffic_queue_capacity: usize,
    #[serde(default = "default_stats_window_hours")]
    pub stats_window_hours: u64,
    #[serde(default = "default_stats_api_enabled")]
    pub stats_api_enabled: bool,
    #[serde(default = "default_stats_api_bind_address")]
    pub stats_api_bind_address: String,
    #[serde(default = "default_stats_api_port")]
    pub stats_api_port: u16,
    #[serde(default)]
    pub api_token: Option<String>,
    /// Dedicated secret used only to encrypt SMTP credentials at rest.
    /// Keep this stable when rotating API authentication tokens.
    #[serde(default)]
    pub smtp_encryption_key: Option<String>,
    #[serde(default = "default_swagger_enabled")]
    pub swagger_enabled: bool,
    #[serde(default = "default_dashboard_enabled")]
    pub dashboard_enabled: bool,
    #[serde(default)]
    pub dashboard_auth: DashboardAuthSettings,
    #[serde(default = "default_base_path")]
    pub base_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardAuthSettings {
    #[serde(default = "default_dashboard_auth_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub roles: Vec<DashboardRoleAssignment>,
    #[serde(default = "default_altcha_enabled")]
    pub altcha_enabled: bool,
    #[serde(default)]
    pub altcha_challenge_url: Option<String>,
    #[serde(default = "default_dashboard_cookie_secure")]
    pub cookie_secure: bool,
    #[serde(default = "default_session_secret")]
    pub session_secret: String,
    #[serde(default = "default_session_duration_hours")]
    pub session_duration_hours: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySettings {
    #[serde(default = "default_telemetry_enabled")]
    pub enabled: bool,
    #[serde(default = "default_telemetry_max_events")]
    pub max_events: usize,
    #[serde(default = "default_telemetry_retention_hours")]
    pub retention_hours: u64,
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            enabled: default_telemetry_enabled(),
            max_events: default_telemetry_max_events(),
            retention_hours: default_telemetry_retention_hours(),
        }
    }
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

fn default_handshake_timeout_ms() -> u64 {
    10_000
}

fn default_tls_enabled() -> bool {
    false
}

fn default_tls_require_client_auth() -> bool {
    false
}

fn default_tls_handshake_timeout_ms() -> u64 {
    10_000
}

fn default_dns_timeout_ms() -> u64 {
    5_000
}

fn default_dns_cache_ttl_secs() -> u64 {
    30
}

fn default_dns_cache_max_entries() -> usize {
    4_096
}

fn default_dns_max_concurrent_lookups() -> usize {
    128
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

fn default_gssapi_service_name() -> String {
    "socks".to_string()
}

fn default_gssapi_protection_level() -> String {
    "integrity".to_string()
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

fn default_session_batch_queue_capacity() -> usize {
    10_000
}

fn default_session_history_max_entries() -> usize {
    100_000
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

fn default_session_traffic_queue_capacity() -> usize {
    10_000
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

fn default_dashboard_auth_enabled() -> bool {
    false
}

fn default_altcha_enabled() -> bool {
    true
}

fn default_dashboard_cookie_secure() -> bool {
    true
}

fn default_session_secret() -> String {
    use base64::engine::general_purpose;
    use base64::Engine;
    use rand::RngCore;

    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn default_session_duration_hours() -> u64 {
    24
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

fn default_telemetry_enabled() -> bool {
    true
}

fn default_telemetry_max_events() -> usize {
    256
}

fn default_telemetry_retention_hours() -> u64 {
    6
}

fn default_pool_max_idle_per_dest() -> usize {
    4
}

fn default_pool_max_total_idle() -> usize {
    100
}

fn default_pool_idle_timeout_secs() -> u64 {
    90
}

fn default_pool_connect_timeout_ms() -> u64 {
    5000
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
            max_connections_per_ip: 0,
            handshake_timeout_ms: default_handshake_timeout_ms(),
            allow_unsafe_public_proxy: false,
            tls: TlsSettings::default(),
            pool: PoolSettings::default(),
            resolver: ResolverSettings::default(),
        }
    }
}

impl Default for ResolverSettings {
    fn default() -> Self {
        Self {
            timeout_ms: default_dns_timeout_ms(),
            cache_ttl_secs: default_dns_cache_ttl_secs(),
            cache_max_entries: default_dns_cache_max_entries(),
            max_concurrent_lookups: default_dns_max_concurrent_lookups(),
        }
    }
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            max_idle_per_dest: default_pool_max_idle_per_dest(),
            max_total_idle: default_pool_max_total_idle(),
            idle_timeout_secs: default_pool_idle_timeout_secs(),
            connect_timeout_ms: default_pool_connect_timeout_ms(),
        }
    }
}

impl From<PoolSettings> for crate::server::pool::PoolConfig {
    fn from(settings: PoolSettings) -> Self {
        Self {
            enabled: settings.enabled,
            max_idle_per_dest: settings.max_idle_per_dest,
            max_total_idle: settings.max_total_idle,
            idle_timeout_secs: settings.idle_timeout_secs,
            connect_timeout_ms: settings.connect_timeout_ms,
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
            handshake_timeout_ms: default_tls_handshake_timeout_ms(),
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
            gssapi: GssApiSettings::default(),
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

impl Default for GssApiSettings {
    fn default() -> Self {
        Self {
            service_name: default_gssapi_service_name(),
            keytab_path: None,
            protection_level: default_gssapi_protection_level(),
            verbose: false,
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
            batch_queue_capacity: default_session_batch_queue_capacity(),
            history_max_entries: default_session_history_max_entries(),
            retention_days: default_session_retention_days(),
            cleanup_interval_hours: default_session_cleanup_interval_hours(),
            traffic_update_packet_interval: default_session_traffic_update_packet_interval(),
            traffic_queue_capacity: default_session_traffic_queue_capacity(),
            stats_window_hours: default_stats_window_hours(),
            stats_api_enabled: default_stats_api_enabled(),
            stats_api_bind_address: default_stats_api_bind_address(),
            stats_api_port: default_stats_api_port(),
            api_token: None,
            smtp_encryption_key: None,
            swagger_enabled: default_swagger_enabled(),
            dashboard_enabled: default_dashboard_enabled(),
            dashboard_auth: DashboardAuthSettings::default(),
            base_path: default_base_path(),
        }
    }
}

impl Default for DashboardAuthSettings {
    fn default() -> Self {
        Self {
            enabled: default_dashboard_auth_enabled(),
            users: Vec::new(),
            roles: Vec::new(),
            altcha_enabled: default_altcha_enabled(),
            altcha_challenge_url: None,
            cookie_secure: default_dashboard_cookie_secure(),
            session_secret: default_session_secret(),
            session_duration_hours: default_session_duration_hours(),
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
    /// Expand environment variable references in sensitive config fields.
    /// Supports `${VAR_NAME}` syntax. If the variable is not set, the value is left unchanged.
    fn expand_env_vars(&mut self) {
        fn expand(value: &mut String) {
            if let Some(var_name) = value.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
                if let Ok(env_val) = std::env::var(var_name) {
                    *value = env_val;
                }
            }
        }

        fn expand_opt(value: &mut Option<String>) {
            if let Some(inner) = value.as_mut() {
                expand(inner);
            }
        }

        expand_opt(&mut self.sessions.database_url);
        expand_opt(&mut self.sessions.api_token);
        expand_opt(&mut self.policy_state.redis_url);
        expand_opt(&mut self.sessions.smtp_encryption_key);
        expand(&mut self.sessions.dashboard_auth.session_secret);
        for user in &mut self.sessions.dashboard_auth.users {
            expand(&mut user.password);
        }
        for user in &mut self.auth.users {
            expand(&mut user.password);
        }
        expand_opt(&mut self.server.tls.key_password);
    }

    /// Load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| RustSocksError::Config(format!("Failed to read config file: {}", e)))?;

        Self::from_toml_str(&content)
    }

    /// Parse configuration from a TOML string with validation and normalization.
    pub fn from_toml_str(content: &str) -> Result<Self> {
        let mut config: Config = toml::from_str(content)
            .map_err(|e| RustSocksError::Config(format!("Failed to parse config: {}", e)))?;

        config.expand_env_vars();
        config.validate()?;

        // Normalize base path after validation so downstream components can rely on canonical form.
        let normalized_base = config.sessions.normalized_base_path();
        config.sessions.base_path = normalized_base;

        Ok(config)
    }

    /// Persist the configuration atomically to the provided path.
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let rendered = toml::to_string_pretty(self).map_err(|e| {
            RustSocksError::Config(format!("Failed to serialize config to TOML: {}", e))
        })? + "\n";

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                RustSocksError::Config(format!(
                    "Failed to create parent directory for config: {}",
                    e
                ))
            })?;
        }

        let timestamp = Utc::now().format("%Y%m%d%H%M%S");
        let tmp_name = format!(
            "{}.tmp.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("rustsocks.toml"),
            timestamp
        );
        let tmp_path = path
            .parent()
            .map(|parent| parent.join(&tmp_name))
            .unwrap_or_else(|| PathBuf::from(&tmp_name));

        {
            #[cfg(unix)]
            let tmp_file = {
                use std::os::unix::fs::OpenOptionsExt;
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp_path)
            };
            #[cfg(not(unix))]
            let tmp_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path);

            let mut tmp_file = tmp_file.map_err(|e| {
                RustSocksError::Config(format!(
                    "Failed to create temporary config file {:?}: {}",
                    tmp_path, e
                ))
            })?;
            tmp_file.write_all(rendered.as_bytes()).map_err(|e| {
                RustSocksError::Config(format!(
                    "Failed to write temporary config file {:?}: {}",
                    tmp_path, e
                ))
            })?;
            tmp_file.flush().map_err(|e| {
                RustSocksError::Config(format!(
                    "Failed to flush temporary config file {:?}: {}",
                    tmp_path, e
                ))
            })?;
            tmp_file.sync_all().map_err(|e| {
                RustSocksError::Config(format!(
                    "Failed to sync temporary config file {:?}: {}",
                    tmp_path, e
                ))
            })?;
        }

        if let Err(err) = fs::rename(&tmp_path, path) {
            if path.exists() {
                fs::remove_file(path).map_err(|e| {
                    RustSocksError::Config(format!(
                        "Failed to remove old config file {:?}: {}",
                        path, e
                    ))
                })?;
            }
            fs::rename(&tmp_path, path).map_err(|e| {
                RustSocksError::Config(format!(
                    "Failed to replace config file {:?}: {} (previous error: {})",
                    path, e, err
                ))
            })?;
        }

        Ok(())
    }

    /// Validate the configuration (public wrapper used by APIs)
    /// The log level to use: the command-line value if given, otherwise `logging.level`.
    pub fn effective_log_level(&self, cli_override: Option<&str>) -> String {
        match cli_override.map(str::trim) {
            Some(level) if !level.is_empty() => level.to_string(),
            _ => self.logging.level.clone(),
        }
    }

    fn validate_policy_state(&self) -> Result<()> {
        let state = &self.policy_state;
        match state.backend.as_str() {
            "memory" => {}
            "redis" => {
                if !cfg!(feature = "redis") {
                    return Err(RustSocksError::Config(
                        "policy_state.backend = \"redis\" requires building with the `redis` feature"
                            .to_string(),
                    ));
                }
                let url = state.redis_url.as_deref().unwrap_or("").trim();
                if url.is_empty() {
                    return Err(RustSocksError::Config(
                        "policy_state.redis_url must be set when policy_state.backend is \"redis\""
                            .to_string(),
                    ));
                }
                if !(url.starts_with("redis://")
                    || url.starts_with("rediss://")
                    || url.starts_with("redis+unix://")
                    || url.starts_with("unix://"))
                {
                    return Err(RustSocksError::Config(
                        "policy_state.redis_url must start with redis://, rediss:// or unix://"
                            .to_string(),
                    ));
                }
            }
            other => {
                return Err(RustSocksError::Config(format!(
                    "policy_state.backend must be \"memory\" or \"redis\", got \"{other}\""
                )));
            }
        }
        if !matches!(state.failure_mode.as_str(), "fail_open" | "fail_closed") {
            return Err(RustSocksError::Config(
                "policy_state.failure_mode must be \"fail_open\" or \"fail_closed\"".to_string(),
            ));
        }
        if state.key_prefix.trim().is_empty()
            || state
                .key_prefix
                .contains(|c: char| c.is_whitespace() || c == '{' || c == '}')
        {
            return Err(RustSocksError::Config(
                "policy_state.key_prefix must be non-empty and contain no whitespace or braces"
                    .to_string(),
            ));
        }
        if !(5..=3600).contains(&state.lease_secs) {
            return Err(RustSocksError::Config(
                "policy_state.lease_secs must be between 5 and 3600".to_string(),
            ));
        }
        if !(10..=10_000).contains(&state.operation_timeout_ms) {
            return Err(RustSocksError::Config(
                "policy_state.operation_timeout_ms must be between 10 and 10000".to_string(),
            ));
        }
        Ok(())
    }

    pub fn validate_effective(&self) -> Result<()> {
        self.validate()
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

        let socks_method_valid = matches!(
            self.auth.socks_method.as_str(),
            "none" | "userpass" | "pam.address" | "pam.username"
        ) || cfg!(feature = "gssapi")
            && self.auth.socks_method == "gssapi";

        if !socks_method_valid {
            let supported = if cfg!(feature = "gssapi") {
                "none, userpass, pam.address, pam.username, gssapi"
            } else {
                "none, userpass, pam.address, pam.username (gssapi requires the gssapi feature)"
            };
            return Err(RustSocksError::Config(format!(
                "Invalid SOCKS auth method: {}. Supported: {}",
                self.auth.socks_method, supported
            )));
        }

        fn unresolved_env(value: &str) -> bool {
            value.starts_with("${") && value.ends_with('}')
        }

        for user in &self.auth.users {
            if unresolved_env(&user.password) {
                return Err(RustSocksError::Config(format!(
                    "Environment variable for auth.users password of '{}' is not set",
                    user.username
                )));
            }
        }
        for user in &self.sessions.dashboard_auth.users {
            if unresolved_env(&user.password) {
                return Err(RustSocksError::Config(format!(
                    "Environment variable for dashboard password of '{}' is not set",
                    user.username
                )));
            }
        }
        if unresolved_env(&self.sessions.dashboard_auth.session_secret) {
            return Err(RustSocksError::Config(
                "Environment variable for sessions.dashboard_auth.session_secret is not set"
                    .to_string(),
            ));
        }
        for (name, value) in [
            (
                "sessions.database_url",
                self.sessions.database_url.as_deref(),
            ),
            ("sessions.api_token", self.sessions.api_token.as_deref()),
            (
                "policy_state.redis_url",
                self.policy_state.redis_url.as_deref(),
            ),
            (
                "sessions.smtp_encryption_key",
                self.sessions.smtp_encryption_key.as_deref(),
            ),
            (
                "server.tls.key_password",
                self.server.tls.key_password.as_deref(),
            ),
        ] {
            if value.map(unresolved_env).unwrap_or(false) {
                return Err(RustSocksError::Config(format!(
                    "Environment variable referenced by {} is not set",
                    name
                )));
            }
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

        // Ensure anonymous_user doesn't collide with a real username
        if self.auth.socks_method == "userpass"
            && self
                .auth
                .users
                .iter()
                .any(|u| u.username == self.acl.anonymous_user)
        {
            return Err(RustSocksError::Config(format!(
                "auth.users contains a user named '{}' which collides with acl.anonymous_user",
                self.acl.anonymous_user
            )));
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

        let db_backed_storage = matches!(
            self.sessions.storage.as_str(),
            "sqlite" | "mariadb" | "mysql"
        );

        if !matches!(
            self.sessions.storage.as_str(),
            "memory" | "sqlite" | "mariadb" | "mysql"
        ) {
            return Err(RustSocksError::Config(format!(
                "Invalid session storage: {}. Supported: memory, sqlite, mariadb, mysql",
                self.sessions.storage
            )));
        }

        if self.sessions.enabled && db_backed_storage && self.sessions.database_url.is_none() {
            return Err(RustSocksError::Config(
                "sessions.database_url is required when session tracking uses sqlite, MariaDB, or MySQL storage"
                    .to_string(),
            ));
        }

        if self.sessions.cleanup_interval_hours == 0 {
            return Err(RustSocksError::Config(
                "sessions.cleanup_interval_hours must be greater than 0".to_string(),
            ));
        }

        if self.sessions.batch_size == 0 {
            return Err(RustSocksError::Config(
                "sessions.batch_size must be greater than 0".to_string(),
            ));
        }

        if self.sessions.batch_interval_ms == 0 {
            return Err(RustSocksError::Config(
                "sessions.batch_interval_ms must be greater than 0".to_string(),
            ));
        }

        if self.sessions.traffic_queue_capacity == 0 {
            return Err(RustSocksError::Config(
                "sessions.traffic_queue_capacity must be greater than 0".to_string(),
            ));
        }

        if self.sessions.batch_queue_capacity == 0 {
            return Err(RustSocksError::Config(
                "sessions.batch_queue_capacity must be greater than 0".to_string(),
            ));
        }

        if self.sessions.history_max_entries == 0 {
            return Err(RustSocksError::Config(
                "sessions.history_max_entries must be greater than 0".to_string(),
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

        if let Some(token) = &self.sessions.api_token {
            if token.trim().is_empty() {
                return Err(RustSocksError::Config(
                    "sessions.api_token cannot be empty when provided".to_string(),
                ));
            }
        }

        if let Some(key) = &self.sessions.smtp_encryption_key {
            if key.trim().len() < 32 {
                return Err(RustSocksError::Config(
                    "sessions.smtp_encryption_key must contain at least 32 non-whitespace characters when provided".to_string(),
                ));
            }
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

        if self.server.bind_address.parse::<IpAddr>().is_err() {
            return Err(RustSocksError::Config(format!(
                "Invalid server.bind_address '{}': expected IPv4 or IPv6 literal",
                self.server.bind_address
            )));
        }

        let bind_ip: IpAddr = self.server.bind_address.parse().expect("validated above");
        let public_no_auth = !bind_ip.is_loopback()
            && self.auth.client_method == "none"
            && self.auth.socks_method == "none";
        if public_no_auth && !self.server.allow_unsafe_public_proxy {
            return Err(RustSocksError::Config(
                "Refusing to expose an unauthenticated SOCKS proxy on a non-loopback address. Configure authentication, bind to loopback, or explicitly set server.allow_unsafe_public_proxy = true.".to_string(),
            ));
        }

        self.validate_policy_state()?;

        if self.server.max_connections_per_ip > self.server.max_connections {
            return Err(RustSocksError::Config(
                "server.max_connections_per_ip must not exceed server.max_connections".to_string(),
            ));
        }

        if self.server.max_connections == 0 {
            return Err(RustSocksError::Config(
                "server.max_connections must be greater than 0".to_string(),
            ));
        }

        if self.server.handshake_timeout_ms == 0 {
            return Err(RustSocksError::Config(
                "server.handshake_timeout_ms must be greater than 0".to_string(),
            ));
        }

        if self.server.handshake_timeout_ms > 300_000 {
            return Err(RustSocksError::Config(
                "server.handshake_timeout_ms must not exceed 300000 (5 minutes)".to_string(),
            ));
        }

        if self.server.tls.handshake_timeout_ms == 0
            || self.server.tls.handshake_timeout_ms > 300_000
        {
            return Err(RustSocksError::Config(
                "server.tls.handshake_timeout_ms must be between 1 and 300000 milliseconds"
                    .to_string(),
            ));
        }

        if self.server.resolver.timeout_ms == 0 || self.server.resolver.timeout_ms > 60_000 {
            return Err(RustSocksError::Config(
                "server.resolver.timeout_ms must be between 1 and 60000 milliseconds".to_string(),
            ));
        }
        if self.server.resolver.cache_ttl_secs == 0 || self.server.resolver.cache_ttl_secs > 86_400
        {
            return Err(RustSocksError::Config(
                "server.resolver.cache_ttl_secs must be between 1 and 86400 seconds".to_string(),
            ));
        }
        if self.server.resolver.cache_max_entries == 0
            || self.server.resolver.cache_max_entries > 1_000_000
        {
            return Err(RustSocksError::Config(
                "server.resolver.cache_max_entries must be between 1 and 1000000".to_string(),
            ));
        }
        if self.server.resolver.max_concurrent_lookups == 0
            || self.server.resolver.max_concurrent_lookups > 4_096
        {
            return Err(RustSocksError::Config(
                "server.resolver.max_concurrent_lookups must be between 1 and 4096".to_string(),
            ));
        }

        if self.server.pool.enabled {
            if self.server.pool.max_idle_per_dest == 0 {
                return Err(RustSocksError::Config(
                    "server.pool.max_idle_per_dest must be greater than 0 when pooling is enabled"
                        .to_string(),
                ));
            }
            if self.server.pool.max_total_idle == 0 {
                return Err(RustSocksError::Config(
                    "server.pool.max_total_idle must be greater than 0 when pooling is enabled"
                        .to_string(),
                ));
            }
            if self.server.pool.max_idle_per_dest > self.server.pool.max_total_idle {
                return Err(RustSocksError::Config(
                    "server.pool.max_idle_per_dest must not exceed server.pool.max_total_idle"
                        .to_string(),
                ));
            }
            if self.server.pool.connect_timeout_ms == 0
                || self.server.pool.connect_timeout_ms > 300_000
            {
                return Err(RustSocksError::Config(
                    "server.pool.connect_timeout_ms must be between 1 and 300000 milliseconds when pooling is enabled".to_string(),
                ));
            }
            if self.server.pool.idle_timeout_secs == 0
                || self.server.pool.idle_timeout_secs > 86_400
            {
                return Err(RustSocksError::Config(
                    "server.pool.idle_timeout_secs must be between 1 and 86400 seconds when pooling is enabled".to_string(),
                ));
            }
        }

        if self.sessions.batch_queue_capacity < self.sessions.batch_size {
            return Err(RustSocksError::Config(
                "sessions.batch_queue_capacity must be greater than or equal to sessions.batch_size".to_string(),
            ));
        }

        if self.qos.enabled {
            if self.qos.algorithm != "htb" {
                return Err(RustSocksError::Config(format!(
                    "Invalid qos.algorithm '{}'. Supported value when QoS is enabled: htb",
                    self.qos.algorithm
                )));
            }
            let htb = &self.qos.htb;
            if htb.global_bandwidth_bytes_per_sec == 0
                || htb.guaranteed_bandwidth_bytes_per_sec == 0
                || htb.max_bandwidth_bytes_per_sec == 0
                || htb.burst_size_bytes == 0
                || htb.refill_interval_ms == 0
                || htb.idle_timeout_secs == 0
            {
                return Err(RustSocksError::Config(
                    "QoS bandwidth, burst, refill interval, and idle timeout values must be greater than 0".to_string(),
                ));
            }
            if htb.guaranteed_bandwidth_bytes_per_sec > htb.max_bandwidth_bytes_per_sec {
                return Err(RustSocksError::Config(
                    "qos.htb.guaranteed_bandwidth_bytes_per_sec must not exceed qos.htb.max_bandwidth_bytes_per_sec".to_string(),
                ));
            }
            if htb.max_bandwidth_bytes_per_sec > htb.global_bandwidth_bytes_per_sec {
                return Err(RustSocksError::Config(
                    "qos.htb.max_bandwidth_bytes_per_sec must not exceed qos.htb.global_bandwidth_bytes_per_sec".to_string(),
                ));
            }
            if htb.fair_sharing_enabled && htb.rebalance_interval_ms == 0 {
                return Err(RustSocksError::Config(
                    "qos.htb.rebalance_interval_ms must be greater than 0 when fair sharing is enabled".to_string(),
                ));
            }
            let limits = &self.qos.connection_limits;
            if limits.max_connections_per_user == 0 || limits.max_connections_global == 0 {
                return Err(RustSocksError::Config(
                    "QoS connection limits must be greater than 0".to_string(),
                ));
            }
            if limits.max_connections_per_user > limits.max_connections_global {
                return Err(RustSocksError::Config(
                    "qos.connection_limits.max_connections_per_user must not exceed max_connections_global".to_string(),
                ));
            }
        }

        if self.sessions.stats_api_enabled {
            if self.sessions.stats_api_bind_address.trim().is_empty() {
                return Err(RustSocksError::Config(
                    "sessions.stats_api_bind_address cannot be empty when stats API is enabled"
                        .to_string(),
                ));
            }

            let stats_ip = self
                .sessions
                .stats_api_bind_address
                .parse::<IpAddr>()
                .map_err(|_| {
                    RustSocksError::Config(format!(
                        "Invalid stats API bind address: {}",
                        self.sessions.stats_api_bind_address
                    ))
                })?;
            let _stats_addr = SocketAddr::new(stats_ip, self.sessions.stats_api_port);
        }

        let normalized_base = self.sessions.normalized_base_path();
        if !normalized_base.starts_with('/') {
            return Err(RustSocksError::Config(
                "sessions.base_path must resolve to an absolute path starting with '/'".to_string(),
            ));
        }

        if self.sessions.dashboard_auth.enabled {
            if self.sessions.dashboard_auth.users.is_empty() {
                return Err(RustSocksError::Config(
                    "sessions.dashboard_auth requires at least one user when enabled".to_string(),
                ));
            }

            for user in &self.sessions.dashboard_auth.users {
                if user.username.trim().is_empty() {
                    return Err(RustSocksError::Config(
                        "sessions.dashboard_auth user username cannot be empty".to_string(),
                    ));
                }
                if user.password.trim().is_empty() {
                    return Err(RustSocksError::Config(
                        "sessions.dashboard_auth user password cannot be empty".to_string(),
                    ));
                }
            }

            for assignment in &self.sessions.dashboard_auth.roles {
                if !matches!(assignment.role.as_str(), "viewer" | "operator" | "admin") {
                    return Err(RustSocksError::Config(format!(
                        "Invalid dashboard role '{}' for '{}'. Supported: viewer, operator, admin",
                        assignment.role, assignment.username
                    )));
                }
                if !self
                    .sessions
                    .dashboard_auth
                    .users
                    .iter()
                    .any(|user| user.username == assignment.username)
                {
                    return Err(RustSocksError::Config(format!(
                        "Dashboard role assignment references unknown user '{}'",
                        assignment.username
                    )));
                }
            }
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
handshake_timeout_ms = 10000

[server.tls]
enabled = false
certificate_path = "config/server.crt"
private_key_path = "config/server.key"
require_client_auth = false
# client_ca_path = "config/ca.crt"
# alpn_protocols = ["socks"]
# min_protocol_version = "TLS13"

[server.pool]
enabled = false
max_idle_per_dest = 4
max_total_idle = 100
idle_timeout_secs = 90
connect_timeout_ms = 5000

[auth]
client_method = "none"       # Options: "none", "pam.address"
socks_method = "none"        # Options: "none", "userpass", "pam.address", "pam.username", "gssapi"

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

[auth.gssapi]
service_name = "socks"
# keytab_path = "/etc/krb5.keytab"
protection_level = "integrity"
verbose = false

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
storage = "memory"  # Options: "memory", "sqlite", "mariadb", "mysql"
# database_url = "sqlite://var/lib/rustsocks/sessions.db"
# database_url = "mysql://user:pass@host:3306/rustsocks_sessions"
batch_size = 100
batch_interval_ms = 1000
retention_days = 90
cleanup_interval_hours = 24
traffic_update_packet_interval = 10
traffic_queue_capacity = 10000
stats_window_hours = 24
stats_api_enabled = false
stats_api_bind_address = "127.0.0.1"
stats_api_port = 9090
# api_token = "change-me"
# smtp_encryption_key = "use-a-stable-random-secret-of-at-least-32-characters"
swagger_enabled = true
dashboard_enabled = false
base_path = "/"

[sessions.dashboard_auth]
enabled = false
# cookie_secure = true
# [[sessions.dashboard_auth.users]]
# username = "admin"
# password = "strong-secret"

[metrics]
enabled = true              # Enable metrics collection
storage = "memory"          # Options: "memory", "sqlite" (uses sessions.database_url)
retention_hours = 24        # Keep metrics for 24 hours
cleanup_interval_hours = 6  # Cleanup old metrics every 6 hours
collection_interval_secs = 5  # Collect metrics every 5 seconds

[telemetry]
enabled = true
max_events = 256
retention_hours = 6

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
        assert_eq!(config.server.handshake_timeout_ms, 10_000);
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
        assert_eq!(config.sessions.traffic_queue_capacity, 10_000);
        assert_eq!(config.sessions.stats_window_hours, 24);
        assert!(!config.sessions.stats_api_enabled);
        assert_eq!(config.sessions.stats_api_bind_address, "127.0.0.1");
        assert_eq!(config.sessions.stats_api_port, 9090);
        assert!(config.sessions.api_token.is_none());
        assert!(config.sessions.swagger_enabled);
        assert!(!config.sessions.dashboard_enabled);
        assert!(!config.sessions.dashboard_auth.enabled);
        assert!(config.sessions.dashboard_auth.users.is_empty());
        assert!(config.sessions.dashboard_auth.cookie_secure);
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

        config.sessions.traffic_queue_capacity = 0;
        assert!(config.validate().is_err());

        config.sessions.traffic_queue_capacity = 1000;
        assert!(config.validate().is_ok());

        config.server.max_connections = 0;
        assert!(config.validate().is_err());

        config.server.max_connections = 100;
        config.server.handshake_timeout_ms = 0;
        assert!(config.validate().is_err());

        config.server.handshake_timeout_ms = 5000;
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

        {
            let mut config = Config::default();
            config.sessions.dashboard_auth.enabled = true;
            assert!(config.validate().is_err());

            config.sessions.dashboard_auth.users.push(User {
                username: "admin".to_string(),
                password: "secret".to_string(),
            });
            assert!(config.validate().is_ok());
        }

        {
            let mut config = Config::default();
            config.sessions.dashboard_auth.enabled = true;
            config.sessions.dashboard_auth.users.push(User {
                username: "".to_string(),
                password: "secret".to_string(),
            });
            assert!(config.validate().is_err());
        }

        {
            let mut config = Config::default();
            config.sessions.dashboard_auth.enabled = true;
            config.sessions.dashboard_auth.users.push(User {
                username: "admin".to_string(),
                password: "".to_string(),
            });
            assert!(config.validate().is_err());
        }
    }
}
