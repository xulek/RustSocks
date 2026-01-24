use rustsocks::config::Config;
use rustsocks::utils::error::RustSocksError;

fn assert_config_error(config: &Config, expected: &str) {
    match config.validate_effective() {
        Ok(_) => panic!("expected config validation error"),
        Err(RustSocksError::Config(msg)) => assert_eq!(msg, expected),
        Err(err) => panic!("unexpected error: {}", err),
    }
}

#[test]
fn tls_requires_certificate_and_key_paths() {
    let mut config = Config::default();
    config.server.tls.enabled = true;

    assert_config_error(
        &config,
        "server.tls.enabled is true but certificate_path is not set",
    );

    config.server.tls.certificate_path = Some("cert.pem".to_string());
    assert_config_error(
        &config,
        "server.tls.enabled is true but private_key_path is not set",
    );
}

#[test]
fn tls_requires_client_ca_when_client_auth_enabled() {
    let mut config = Config::default();
    config.server.tls.enabled = true;
    config.server.tls.certificate_path = Some("cert.pem".to_string());
    config.server.tls.private_key_path = Some("key.pem".to_string());
    config.server.tls.require_client_auth = true;

    assert_config_error(
        &config,
        "server.tls.client_ca_path is required when require_client_auth is true",
    );
}

#[test]
fn tls_rejects_invalid_min_protocol_version() {
    let mut config = Config::default();
    config.server.tls.enabled = true;
    config.server.tls.certificate_path = Some("cert.pem".to_string());
    config.server.tls.private_key_path = Some("key.pem".to_string());
    config.server.tls.min_protocol_version = Some("TLS11".to_string());

    assert_config_error(
        &config,
        "Invalid server.tls.min_protocol_version 'TLS11'. Supported values: TLS12, TLS13",
    );
}

#[test]
fn stats_api_bind_address_must_be_valid() {
    let mut config = Config::default();
    config.sessions.stats_api_enabled = true;
    config.sessions.stats_api_bind_address = "not-an-ip".to_string();

    assert_config_error(
        &config,
        "Invalid stats API bind address/port: not-an-ip:9090",
    );
}

#[test]
fn sessions_api_token_cannot_be_empty() {
    let mut config = Config::default();
    config.sessions.api_token = Some("   ".to_string());

    assert_config_error(&config, "sessions.api_token cannot be empty when provided");
}

#[test]
fn sessions_traffic_update_interval_must_be_positive() {
    let mut config = Config::default();
    config.sessions.traffic_update_packet_interval = 0;

    assert_config_error(
        &config,
        "sessions.traffic_update_packet_interval must be greater than 0",
    );
}

#[test]
fn metrics_storage_must_be_valid() {
    let mut config = Config::default();
    config.metrics.storage = "filesystem".to_string();

    assert_config_error(
        &config,
        "Invalid metrics storage: filesystem. Supported: memory, sqlite",
    );
}

#[test]
fn metrics_cleanup_interval_must_be_positive() {
    let mut config = Config::default();
    config.metrics.cleanup_interval_hours = 0;

    assert_config_error(
        &config,
        "metrics.cleanup_interval_hours must be greater than 0",
    );
}

#[test]
fn metrics_collection_interval_must_be_positive() {
    let mut config = Config::default();
    config.metrics.collection_interval_secs = 0;

    assert_config_error(
        &config,
        "metrics.collection_interval_secs must be greater than 0",
    );
}

#[test]
fn from_toml_str_normalizes_base_path() {
    let toml = r#"
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 1000
handshake_timeout_ms = 10000

[auth]
client_method = "none"
socks_method = "none"

[sessions]
base_path = "///api//v1/"
"#;

    let config = Config::from_toml_str(toml).expect("valid config");
    assert_eq!(config.sessions.base_path, "/api/v1");
}
