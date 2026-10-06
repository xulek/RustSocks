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

    assert_config_error(&config, "Invalid stats API bind address: not-an-ip");
}

#[test]
fn stats_api_bind_address_accepts_ipv6() {
    let mut config = Config::default();
    config.sessions.stats_api_enabled = true;
    config.sessions.stats_api_bind_address = "::1".to_string();

    assert!(config.validate_effective().is_ok());
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

#[test]
fn per_ip_connection_limit_defaults_to_disabled_and_is_bounded_by_global_limit() {
    let mut config = Config::default();
    assert_eq!(config.server.max_connections_per_ip, 0);
    assert!(config.validate_effective().is_ok());

    config.server.max_connections = 100;
    config.server.max_connections_per_ip = 100;
    assert!(config.validate_effective().is_ok());

    config.server.max_connections_per_ip = 101;
    assert_config_error(
        &config,
        "server.max_connections_per_ip must not exceed server.max_connections",
    );
}

#[test]
fn per_ip_connection_limit_is_read_from_toml() {
    let toml = r#"
[server]
bind_address = "127.0.0.1"
bind_port = 1080
max_connections = 500
max_connections_per_ip = 25

[auth]
client_method = "none"
socks_method = "none"
"#;
    let config = Config::from_toml_str(toml).expect("config parses");
    assert_eq!(config.server.max_connections_per_ip, 25);
}

#[test]
fn policy_state_defaults_to_memory_backend() {
    let config = Config::default();
    assert_eq!(config.policy_state.backend, "memory");
    assert_eq!(config.policy_state.failure_mode, "fail_closed");
    assert!(config.validate_effective().is_ok());
}

#[test]
fn policy_state_rejects_unknown_backend_and_failure_mode() {
    let mut config = Config::default();
    config.policy_state.backend = "etcd".to_string();
    assert_config_error(
        &config,
        "policy_state.backend must be \"memory\" or \"redis\", got \"etcd\"",
    );

    let mut config = Config::default();
    config.policy_state.failure_mode = "ignore".to_string();
    assert_config_error(
        &config,
        "policy_state.failure_mode must be \"fail_open\" or \"fail_closed\"",
    );
}

#[test]
fn policy_state_validates_tuning_values() {
    let mut config = Config::default();
    config.policy_state.lease_secs = 2;
    assert_config_error(
        &config,
        "policy_state.lease_secs must be between 5 and 3600",
    );

    let mut config = Config::default();
    config.policy_state.operation_timeout_ms = 5;
    assert_config_error(
        &config,
        "policy_state.operation_timeout_ms must be between 10 and 10000",
    );

    let mut config = Config::default();
    config.policy_state.key_prefix = "bad{prefix}".to_string();
    assert_config_error(
        &config,
        "policy_state.key_prefix must be non-empty and contain no whitespace or braces",
    );
}

#[test]
fn policy_state_redis_backend_requires_feature_and_url() {
    let mut config = Config::default();
    config.policy_state.backend = "redis".to_string();

    if cfg!(feature = "redis") {
        assert_config_error(
            &config,
            "policy_state.redis_url must be set when policy_state.backend is \"redis\"",
        );

        config.policy_state.redis_url = Some("http://localhost".to_string());
        assert_config_error(
            &config,
            "policy_state.redis_url must start with redis://, rediss:// or unix://",
        );

        config.policy_state.redis_url = Some("redis://127.0.0.1:6379/0".to_string());
        assert!(config.validate_effective().is_ok());
    } else {
        config.policy_state.redis_url = Some("redis://127.0.0.1:6379/0".to_string());
        assert_config_error(
            &config,
            "policy_state.backend = \"redis\" requires building with the `redis` feature",
        );
    }
}

#[test]
fn policy_state_is_read_from_toml() {
    let toml = r#"
[server]
bind_address = "127.0.0.1"
bind_port = 1080

[auth]
client_method = "none"
socks_method = "none"

[policy_state]
backend = "memory"
key_prefix = "prod-eu"
failure_mode = "fail_open"
lease_secs = 30
"#;
    let config = Config::from_toml_str(toml).expect("config parses");
    assert_eq!(config.policy_state.key_prefix, "prod-eu");
    assert_eq!(config.policy_state.failure_mode, "fail_open");
    assert_eq!(config.policy_state.lease_secs, 30);
}

#[test]
fn log_level_comes_from_config_unless_overridden_on_the_command_line() {
    let toml = r#"
[server]
bind_address = "127.0.0.1"
bind_port = 1080

[auth]
client_method = "none"
socks_method = "none"

[logging]
level = "warn"
format = "json"
"#;
    let config = Config::from_toml_str(toml).expect("config parses");
    assert_eq!(config.logging.level, "warn");
    assert_eq!(config.logging.format, "json");

    // No CLI value: the configured level applies (it used to be ignored).
    assert_eq!(config.effective_log_level(None), "warn");
    assert_eq!(config.effective_log_level(Some("")), "warn");
    assert_eq!(config.effective_log_level(Some("  ")), "warn");
    // The command line wins when given.
    assert_eq!(config.effective_log_level(Some("debug")), "debug");

    // Defaults stay "info" when the config says nothing.
    assert_eq!(Config::default().effective_log_level(None), "info");
}
