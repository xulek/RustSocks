use crate::api::handlers::sessions::ApiState;
use crate::api::types::{AclTestRequest, AclTestResponse, HealthResponse};
use crate::config::Config;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::process::{self, Command};
use std::sync::Arc;
use tokio::fs;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

/// GET /health - Health check endpoint
pub async fn health_check(State(state): State<ApiState>) -> (StatusCode, Json<HealthResponse>) {
    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
    };

    (StatusCode::OK, Json(response))
}

#[derive(Serialize, Deserialize)]
pub struct ReloadResponse {
    pub success: bool,
    pub message: String,
}

/// POST /api/admin/reload-acl - Reload ACL configuration
pub async fn reload_acl(State(state): State<ApiState>) -> (StatusCode, Json<ReloadResponse>) {
    // Check if ACL is enabled
    let Some(ref acl_engine) = state.acl_engine else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ReloadResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
            }),
        );
    };

    let Some(ref config_path) = state.acl_config_path else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ReloadResponse {
                success: false,
                message: "ACL config path not set".to_string(),
            }),
        );
    };

    // Load new config from file
    let new_config = match crate::acl::load_acl_config(config_path).await {
        Ok(config) => config,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReloadResponse {
                    success: false,
                    message: format!("Failed to load ACL config: {}", e),
                }),
            );
        }
    };

    // Reload ACL engine
    match acl_engine.reload(new_config).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ReloadResponse {
                success: true,
                message: "ACL reloaded successfully".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ReloadResponse {
                success: false,
                message: format!("Failed to reload ACL: {}", e),
            }),
        ),
    }
}

#[derive(Serialize)]
pub struct ConfigFileResponse {
    pub path: Option<String>,
    pub content: String,
    pub editable: bool,
}

/// GET /api/admin/config-file - Fetch current configuration file content
pub async fn get_config_file(
    State(state): State<ApiState>,
) -> (StatusCode, Json<ConfigFileResponse>) {
    let fallback_content =
        toml::to_string_pretty(&*state.config_snapshot).unwrap_or_else(|_| "".to_string());

    if let Some(path) = state.config_path.clone() {
        let content = match fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => {
                warn!(
                    error = %e,
                    path = %path.display(),
                    "Failed to read configuration file, returning in-memory snapshot"
                );
                fallback_content.clone()
            }
        };

        (
            StatusCode::OK,
            Json(ConfigFileResponse {
                path: Some(path.display().to_string()),
                content,
                editable: true,
            }),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(ConfigFileResponse {
                path: None,
                content: fallback_content,
                editable: false,
            }),
        )
    }
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub content: String,
    #[serde(default = "default_restart_flag")]
    pub restart: bool,
}

fn default_restart_flag() -> bool {
    true
}

#[derive(Serialize)]
pub struct ConfigUpdateResponse {
    pub success: bool,
    pub message: String,
    pub restarting: bool,
}

/// PUT /api/admin/config-file - Update configuration and optionally restart
pub async fn update_config_file(
    State(state): State<ApiState>,
    Json(payload): Json<UpdateConfigRequest>,
) -> (StatusCode, Json<ConfigUpdateResponse>) {
    let Some(path) = state.config_path.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ConfigUpdateResponse {
                success: false,
                message: "Server was started without a config file path; editing is disabled"
                    .to_string(),
                restarting: false,
            }),
        );
    };

    let new_config = match Config::from_toml_str(&payload.content) {
        Ok(cfg) => cfg,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ConfigUpdateResponse {
                    success: false,
                    message: format!("Invalid configuration: {}", e),
                    restarting: false,
                }),
            );
        }
    };

    if let Err(e) = new_config.write_to_file(&path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ConfigUpdateResponse {
                success: false,
                message: format!("Failed to write configuration: {}", e),
                restarting: false,
            }),
        );
    }

    info!(
        path = %path.display(),
        restart = payload.restart,
        "Configuration updated via API"
    );

    if payload.restart {
        schedule_restart(state.original_args.clone());
    }

    (
        StatusCode::OK,
        Json(ConfigUpdateResponse {
            success: true,
            message: "Configuration saved successfully".to_string(),
            restarting: payload.restart,
        }),
    )
}

#[derive(Serialize)]
pub struct RuntimeConfigResponse {
    pub path: Option<String>,
    pub editable: bool,
    pub server: ServerRuntimeConfig,
    pub pool: PoolRuntimeConfig,
    pub sessions: SessionsRuntimeConfig,
    pub metrics: MetricsRuntimeConfig,
    pub telemetry: TelemetryRuntimeConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ServerRuntimeConfig {
    pub bind_address: String,
    pub bind_port: u16,
    pub stats_api_enabled: bool,
    pub stats_api_bind_address: String,
    pub stats_api_port: u16,
    pub dashboard_enabled: bool,
    pub swagger_enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PoolRuntimeConfig {
    pub enabled: bool,
    pub max_idle_per_dest: usize,
    pub max_total_idle: usize,
    pub idle_timeout_secs: u64,
    pub connect_timeout_ms: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionsRuntimeConfig {
    pub enabled: bool,
    pub storage: String,
    pub database_url: Option<String>,
    pub retention_days: u64,
    pub cleanup_interval_hours: u64,
    pub traffic_update_packet_interval: u64,
    pub stats_window_hours: u64,
    pub base_path: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MetricsRuntimeConfig {
    pub enabled: bool,
    pub storage: String,
    pub retention_hours: u64,
    pub cleanup_interval_hours: u64,
    pub collection_interval_secs: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TelemetryRuntimeConfig {
    pub enabled: bool,
    pub max_events: usize,
    pub retention_hours: u64,
}

#[derive(Deserialize)]
pub struct RuntimeConfigUpdateRequest {
    pub server: ServerRuntimeConfig,
    pub pool: PoolRuntimeConfig,
    pub sessions: SessionsRuntimeConfig,
    pub metrics: MetricsRuntimeConfig,
    pub telemetry: TelemetryRuntimeConfig,
    #[serde(default = "default_restart_flag")]
    pub restart: bool,
}

/// GET /api/admin/runtime-config - Fetch structured configuration
pub async fn get_runtime_config(
    State(state): State<ApiState>,
) -> (StatusCode, Json<RuntimeConfigResponse>) {
    let response = RuntimeConfigResponse {
        path: state.config_path.as_ref().map(|p| p.display().to_string()),
        editable: state.config_path.is_some(),
        server: map_server_config(&state.config_snapshot),
        pool: map_pool_config(&state.config_snapshot),
        sessions: map_sessions_config(&state.config_snapshot),
        metrics: map_metrics_config(&state.config_snapshot),
        telemetry: map_telemetry_config(&state.config_snapshot),
    };

    (StatusCode::OK, Json(response))
}

/// PUT /api/admin/runtime-config - Update structured configuration
pub async fn update_runtime_config(
    State(state): State<ApiState>,
    Json(payload): Json<RuntimeConfigUpdateRequest>,
) -> (StatusCode, Json<ConfigUpdateResponse>) {
    let Some(path) = state.config_path.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ConfigUpdateResponse {
                success: false,
                message: "Server was started without a config file path; editing is disabled"
                    .to_string(),
                restarting: false,
            }),
        );
    };

    let mut new_config = (*state.config_snapshot).clone();
    apply_server_config(&mut new_config, &payload.server);
    apply_pool_config(&mut new_config, &payload.pool);
    apply_sessions_config(&mut new_config, &payload.sessions);
    apply_metrics_config(&mut new_config, &payload.metrics);
    apply_telemetry_config(&mut new_config, &payload.telemetry);

    if let Err(e) = new_config.validate_effective() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ConfigUpdateResponse {
                success: false,
                message: format!("Invalid configuration: {}", e),
                restarting: false,
            }),
        );
    }

    if let Err(e) = new_config.write_to_file(&path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ConfigUpdateResponse {
                success: false,
                message: format!("Failed to write configuration: {}", e),
                restarting: false,
            }),
        );
    }

    if payload.restart {
        schedule_restart(state.original_args.clone());
    }

    (
        StatusCode::OK,
        Json(ConfigUpdateResponse {
            success: true,
            message: "Configuration saved successfully".to_string(),
            restarting: payload.restart,
        }),
    )
}

fn map_server_config(cfg: &Config) -> ServerRuntimeConfig {
    ServerRuntimeConfig {
        bind_address: cfg.server.bind_address.clone(),
        bind_port: cfg.server.bind_port,
        stats_api_enabled: cfg.sessions.stats_api_enabled,
        stats_api_bind_address: cfg.sessions.stats_api_bind_address.clone(),
        stats_api_port: cfg.sessions.stats_api_port,
        dashboard_enabled: cfg.sessions.dashboard_enabled,
        swagger_enabled: cfg.sessions.swagger_enabled,
    }
}

fn apply_server_config(cfg: &mut Config, dto: &ServerRuntimeConfig) {
    cfg.server.bind_address = dto.bind_address.clone();
    cfg.server.bind_port = dto.bind_port;
    cfg.sessions.stats_api_enabled = dto.stats_api_enabled;
    cfg.sessions.stats_api_bind_address = dto.stats_api_bind_address.clone();
    cfg.sessions.stats_api_port = dto.stats_api_port;
    cfg.sessions.dashboard_enabled = dto.dashboard_enabled;
    cfg.sessions.swagger_enabled = dto.swagger_enabled;
}

fn map_pool_config(cfg: &Config) -> PoolRuntimeConfig {
    PoolRuntimeConfig {
        enabled: cfg.server.pool.enabled,
        max_idle_per_dest: cfg.server.pool.max_idle_per_dest,
        max_total_idle: cfg.server.pool.max_total_idle,
        idle_timeout_secs: cfg.server.pool.idle_timeout_secs,
        connect_timeout_ms: cfg.server.pool.connect_timeout_ms,
    }
}

fn apply_pool_config(cfg: &mut Config, dto: &PoolRuntimeConfig) {
    cfg.server.pool.enabled = dto.enabled;
    cfg.server.pool.max_idle_per_dest = dto.max_idle_per_dest;
    cfg.server.pool.max_total_idle = dto.max_total_idle;
    cfg.server.pool.idle_timeout_secs = dto.idle_timeout_secs;
    cfg.server.pool.connect_timeout_ms = dto.connect_timeout_ms;
}

fn map_sessions_config(cfg: &Config) -> SessionsRuntimeConfig {
    SessionsRuntimeConfig {
        enabled: cfg.sessions.enabled,
        storage: cfg.sessions.storage.clone(),
        database_url: cfg.sessions.database_url.clone(),
        retention_days: cfg.sessions.retention_days,
        cleanup_interval_hours: cfg.sessions.cleanup_interval_hours,
        traffic_update_packet_interval: cfg.sessions.traffic_update_packet_interval,
        stats_window_hours: cfg.sessions.stats_window_hours,
        base_path: cfg.sessions.base_path.clone(),
    }
}

fn apply_sessions_config(cfg: &mut Config, dto: &SessionsRuntimeConfig) {
    cfg.sessions.enabled = dto.enabled;
    cfg.sessions.storage = dto.storage.clone();
    cfg.sessions.database_url = dto.database_url.clone();
    cfg.sessions.retention_days = dto.retention_days;
    cfg.sessions.cleanup_interval_hours = dto.cleanup_interval_hours;
    cfg.sessions.traffic_update_packet_interval = dto.traffic_update_packet_interval;
    cfg.sessions.stats_window_hours = dto.stats_window_hours;
    cfg.sessions.base_path = dto.base_path.clone();
    cfg.sessions.base_path = cfg.sessions.normalized_base_path();
}

fn map_metrics_config(cfg: &Config) -> MetricsRuntimeConfig {
    MetricsRuntimeConfig {
        enabled: cfg.metrics.enabled,
        storage: cfg.metrics.storage.clone(),
        retention_hours: cfg.metrics.retention_hours,
        cleanup_interval_hours: cfg.metrics.cleanup_interval_hours,
        collection_interval_secs: cfg.metrics.collection_interval_secs,
    }
}

fn apply_metrics_config(cfg: &mut Config, dto: &MetricsRuntimeConfig) {
    cfg.metrics.enabled = dto.enabled;
    cfg.metrics.storage = dto.storage.clone();
    cfg.metrics.retention_hours = dto.retention_hours;
    cfg.metrics.cleanup_interval_hours = dto.cleanup_interval_hours;
    cfg.metrics.collection_interval_secs = dto.collection_interval_secs;
}

fn map_telemetry_config(cfg: &Config) -> TelemetryRuntimeConfig {
    TelemetryRuntimeConfig {
        enabled: cfg.telemetry.enabled,
        max_events: cfg.telemetry.max_events,
        retention_hours: cfg.telemetry.retention_hours,
    }
}

fn apply_telemetry_config(cfg: &mut Config, dto: &TelemetryRuntimeConfig) {
    cfg.telemetry.enabled = dto.enabled;
    cfg.telemetry.max_events = dto.max_events;
    cfg.telemetry.retention_hours = dto.retention_hours;
}

fn schedule_restart(original_args: Arc<Vec<OsString>>) {
    tokio::spawn(async move {
        info!("Restarting RustSocks in 1 second to apply new configuration");
        sleep(Duration::from_secs(1)).await;

        match std::env::current_exe() {
            Ok(exe) => {
                let mut command = Command::new(exe);
                if original_args.len() > 1 {
                    command.args(original_args.iter().skip(1));
                }
                match command.spawn() {
                    Ok(_) => {
                        info!("Spawned new RustSocks process, terminating current instance");
                        process::exit(0);
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to spawn new RustSocks process, aborting restart");
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Cannot determine current executable for restart");
            }
        }
    });
}

#[derive(Serialize, Deserialize)]
pub struct AclRulesResponse {
    pub user_count: usize,
    pub group_count: usize,
    pub message: String,
}

/// GET /api/acl/rules - Get current ACL rules summary
pub async fn get_acl_rules(State(state): State<ApiState>) -> (StatusCode, Json<AclRulesResponse>) {
    let Some(ref acl_engine) = state.acl_engine else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AclRulesResponse {
                user_count: 0,
                group_count: 0,
                message: "ACL is not enabled".to_string(),
            }),
        );
    };

    let user_count = acl_engine.get_user_count().await;
    let group_count = acl_engine.get_group_count().await;

    let response = AclRulesResponse {
        user_count,
        group_count,
        message: format!(
            "ACL has {} users and {} groups configured",
            user_count, group_count
        ),
    };

    (StatusCode::OK, Json(response))
}

/// POST /api/acl/test - Test ACL decision for a connection
pub async fn test_acl_decision(
    State(state): State<ApiState>,
    Json(request): Json<AclTestRequest>,
) -> (StatusCode, Json<AclTestResponse>) {
    let Some(ref acl_engine) = state.acl_engine else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AclTestResponse {
                user: request.user,
                destination: request.destination,
                port: request.port,
                protocol: request.protocol,
                decision: "error".to_string(),
                matched_rule: Some("ACL is not enabled".to_string()),
            }),
        );
    };

    // Parse protocol
    let protocol = match request.protocol.to_lowercase().as_str() {
        "tcp" => crate::acl::Protocol::Tcp,
        "udp" => crate::acl::Protocol::Udp,
        "both" => crate::acl::Protocol::Both,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AclTestResponse {
                    user: request.user,
                    destination: request.destination,
                    port: request.port,
                    protocol: request.protocol,
                    decision: "error".to_string(),
                    matched_rule: Some("Invalid protocol (use: tcp, udp, or both)".to_string()),
                }),
            );
        }
    };

    // Parse destination as Address (IP or domain)
    let address = match request.destination.parse::<std::net::IpAddr>() {
        Ok(ip) => match ip {
            std::net::IpAddr::V4(ipv4) => crate::protocol::Address::IPv4(ipv4.octets()),
            std::net::IpAddr::V6(ipv6) => crate::protocol::Address::IPv6(ipv6.octets()),
        },
        Err(_) => crate::protocol::Address::Domain(request.destination.clone()),
    };

    // Evaluate ACL
    let (decision, matched_rule) = acl_engine
        .evaluate(&request.user, &address, request.port, &protocol)
        .await;

    // Convert decision to string
    let decision_str = match decision {
        crate::acl::AclDecision::Allow => "allow",
        crate::acl::AclDecision::Block => "block",
    };

    let response = AclTestResponse {
        user: request.user,
        destination: request.destination,
        port: request.port,
        protocol: request.protocol,
        decision: decision_str.to_string(),
        matched_rule,
    };

    (StatusCode::OK, Json(response))
}

/// GET /metrics - Prometheus metrics endpoint
pub async fn get_metrics(State(state): State<ApiState>) -> (StatusCode, String) {
    let sessions = state.session_manager.get_all_sessions().await;

    let active_count = sessions
        .iter()
        .filter(|s| s.status.as_str() == "active")
        .count();
    let total_sessions = sessions.len();
    let total_bytes_sent: u64 = sessions.iter().map(|s| s.bytes_sent).sum();
    let total_bytes_received: u64 = sessions.iter().map(|s| s.bytes_received).sum();

    let metrics = format!(
        "# HELP rustsocks_active_sessions Active sessions\n\
         # TYPE rustsocks_active_sessions gauge\n\
         rustsocks_active_sessions {}\n\
         # HELP rustsocks_sessions_total Total sessions\n\
         # TYPE rustsocks_sessions_total counter\n\
         rustsocks_sessions_total {}\n\
         # HELP rustsocks_bytes_sent_total Total bytes sent\n\
         # TYPE rustsocks_bytes_sent_total counter\n\
         rustsocks_bytes_sent_total {}\n\
         # HELP rustsocks_bytes_received_total Total bytes received\n\
         # TYPE rustsocks_bytes_received_total counter\n\
         rustsocks_bytes_received_total {}\n",
        active_count, total_sessions, total_bytes_sent, total_bytes_received
    );

    (StatusCode::OK, metrics)
}
