use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::config::DashboardAuthSettings;
use crate::server::pool::PoolStats;

/// API health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
}

/// Session detail in API response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionResponse {
    pub id: String,
    pub user: String,
    pub source_ip: String,
    pub source_port: u16,
    pub dest_ip: String,
    pub dest_port: u16,
    pub protocol: String,
    pub status: String,
    pub acl_decision: String,
    pub acl_rule: Option<String>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub start_time: String,
    pub end_time: Option<String>,
    pub duration_seconds: Option<u64>,
}

/// Aggregated session statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionStatsResponse {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub closed_sessions: u64,
    pub failed_sessions: u64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub top_users: Vec<UserStat>,
    pub top_destinations: Vec<DestinationStat>,
}

/// Per-user statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct UserStat {
    pub user: String,
    pub session_count: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Per-destination statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct DestinationStat {
    pub destination: String,
    pub session_count: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// ACL decision test response
#[derive(Debug, Serialize, Deserialize)]
pub struct AclTestResponse {
    pub user: String,
    pub destination: String,
    pub port: u16,
    pub protocol: String,
    pub decision: String,
    pub matched_rule: Option<String>,
}

/// ACL rule info for API response
#[derive(Debug, Serialize, Deserialize)]
pub struct AclRuleResponse {
    pub user_or_group: String,
    pub action: String,
    pub description: String,
    pub destinations: Vec<String>,
    pub ports: Vec<String>,
    pub protocols: Vec<String>,
    pub priority: u32,
}

/// Paginated response for list endpoints
#[derive(Debug, Serialize, Deserialize)]
pub struct PagedResponse<T> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// Query parameters for sessions history
#[derive(Debug, Deserialize)]
pub struct SessionQueryParams {
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub hours: Option<u32>,
    #[serde(default)]
    pub dest_ip: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub sort_dir: Option<String>,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    50
}

/// ACL test request
#[derive(Debug, Deserialize)]
pub struct AclTestRequest {
    pub user: String,
    pub destination: String,
    pub port: u16,
    pub protocol: String,
}

/// Connectivity test request payload
#[derive(Debug, Deserialize)]
pub struct ConnectivityTestRequest {
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Connectivity test response payload
#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectivityTestResponse {
    pub address: String,
    pub port: u16,
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub error: Option<String>,
}

/// API error response
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    pub status_code: u16,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>, status_code: u16) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
            status_code,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("NotFound", message, 404)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BadRequest", message, 400)
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("InternalError", message, 500)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("Unauthorized", message, 401)
    }
}

/// API configuration
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind_address: String,
    pub bind_port: u16,
    pub enable_api: bool,
    pub token: Option<String>,
    pub swagger_enabled: bool,
    pub dashboard_enabled: bool,
    pub dashboard_auth: DashboardAuthSettings,
    pub base_path: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 9090,
            enable_api: false,
            token: None,
            swagger_enabled: true,
            dashboard_enabled: false,
            dashboard_auth: DashboardAuthSettings::default(),
            base_path: "/".to_string(),
        }
    }
}

// ============================================================================
// ACL Management API Types
// ============================================================================

/// Request to add a new ACL rule
#[derive(Debug, Serialize, Deserialize)]
pub struct AddRuleRequest {
    pub action: String,
    pub description: String,
    pub destinations: Vec<String>,
    pub ports: Vec<String>,
    pub protocols: Vec<String>,
    pub priority: u32,
}

/// Request to update an existing ACL rule
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateRuleRequest {
    /// Identifier of the rule to update
    #[serde(rename = "match")]
    pub match_rule: RuleIdentifierRequest,
    /// New rule values
    pub update: AddRuleRequest,
}

/// Identifier for finding a specific rule
#[derive(Debug, Serialize, Deserialize)]
pub struct RuleIdentifierRequest {
    pub destinations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<String>>,
}

/// Request to delete a rule
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteRuleRequest {
    pub destinations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<String>>,
}

/// Response for add/update/delete rule operations
#[derive(Debug, Serialize, Deserialize)]
pub struct RuleOperationResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<crate::acl::types::AclRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_rule: Option<crate::acl::types::AclRule>,
}

/// Group summary for list endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupSummary {
    pub name: String,
    pub rule_count: usize,
}

/// Response for GET /api/acl/groups
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupListResponse {
    pub groups: Vec<GroupSummary>,
}

/// Response for GET /api/acl/groups/{groupname}
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupDetailResponse {
    pub name: String,
    pub rules: Vec<crate::acl::types::AclRule>,
}

/// User summary for list endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct UserSummary {
    pub username: String,
    pub groups: Vec<String>,
    pub rule_count: usize,
}

/// Response for GET /api/acl/users
#[derive(Debug, Serialize, Deserialize)]
pub struct UserListResponse {
    pub users: Vec<UserSummary>,
}

/// Response for GET /api/acl/users/{username}
#[derive(Debug, Serialize, Deserialize)]
pub struct UserDetailResponse {
    pub username: String,
    pub groups: Vec<String>,
    pub rules: Vec<crate::acl::types::AclRule>,
}

/// Response for GET /api/acl/global
#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalSettingsResponse {
    pub default_policy: String,
}

/// Request to update global settings
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateGlobalSettingsRequest {
    pub default_policy: String,
}

/// Response for global settings update
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateGlobalSettingsResponse {
    pub success: bool,
    pub message: String,
    pub old_policy: String,
    pub new_policy: String,
}

/// Request to search for rules
#[derive(Debug, Serialize, Deserialize)]
pub struct RuleSearchRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// Single search result
#[derive(Debug, Serialize, Deserialize)]
pub struct RuleSearchResultItem {
    pub rule_type: String, // "group" or "user"
    pub owner: String,     // group name or username
    pub rule: crate::acl::types::AclRule,
}

/// Response for POST /api/acl/search
#[derive(Debug, Serialize, Deserialize)]
pub struct RuleSearchResponse {
    pub matches: Vec<RuleSearchResultItem>,
    pub count: usize,
}

/// Request to create a new group
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
}

/// Response for delete group operation
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteGroupResponse {
    pub success: bool,
    pub message: String,
    pub deleted_group: Option<crate::acl::types::GroupAcl>,
}

/// Request to create a new user
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
}

/// Response for delete user operation
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteUserResponse {
    pub success: bool,
    pub message: String,
    pub deleted_user: Option<crate::acl::types::UserAcl>,
}

/// Request to assign user to group
#[derive(Debug, Serialize, Deserialize)]
pub struct AddUserToGroupRequest {
    pub group_name: String,
}

/// Response for user-group operations
#[derive(Debug, Serialize, Deserialize)]
pub struct UserGroupOperationResponse {
    pub success: bool,
    pub message: String,
}

// ============================================================================
// Connection Pool API Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct PoolStatsResponse {
    pub enabled: bool,
    pub total_idle: usize,
    pub active_in_use: u64,
    pub destinations: usize,
    pub total_created: u64,
    pub total_reused: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub dropped_full: u64,
    pub expired: u64,
    pub evicted: u64,
    pub pending_creates: u64,
    pub hit_rate: f64,
    pub config: PoolConfigResponse,
    pub destinations_breakdown: Vec<PoolDestinationResponse>,
}

#[derive(Debug, Serialize)]
pub struct PoolConfigResponse {
    pub enabled: bool,
    pub max_idle_per_dest: usize,
    pub max_total_idle: usize,
    pub idle_timeout_secs: u64,
    pub connect_timeout_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct PoolDestinationResponse {
    pub destination: String,
    pub idle_connections: usize,
    pub in_use: u64,
    pub total_created: u64,
    pub total_reused: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
    pub drops: u64,
    pub evicted: u64,
    pub expired: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_miss: Option<String>,
}

impl From<PoolStats> for PoolStatsResponse {
    fn from(stats: PoolStats) -> Self {
        let destinations_breakdown = stats
            .per_destination
            .into_iter()
            .map(|dest| PoolDestinationResponse {
                destination: dest.destination.to_string(),
                idle_connections: dest.idle_connections,
                in_use: dest.in_use,
                total_created: dest.total_created,
                total_reused: dest.total_reused,
                pool_hits: dest.pool_hits,
                pool_misses: dest.pool_misses,
                drops: dest.drops,
                evicted: dest.evicted,
                expired: dest.expired,
                last_activity: format_system_time(dest.last_activity),
                last_miss: format_system_time(dest.last_miss),
            })
            .collect();

        let total_checks = stats.pool_hits + stats.pool_misses;
        let hit_rate = if total_checks > 0 {
            stats.pool_hits as f64 / total_checks as f64
        } else {
            0.0
        };

        Self {
            enabled: stats.config.enabled,
            total_idle: stats.total_idle,
            active_in_use: stats.connections_in_use,
            destinations: stats.destinations,
            total_created: stats.total_created,
            total_reused: stats.total_reused,
            pool_hits: stats.pool_hits,
            pool_misses: stats.pool_misses,
            dropped_full: stats.dropped_full,
            expired: stats.expired,
            evicted: stats.evicted,
            pending_creates: stats.pending_creates,
            hit_rate,
            config: PoolConfigResponse {
                enabled: stats.config.enabled,
                max_idle_per_dest: stats.config.max_idle_per_dest,
                max_total_idle: stats.config.max_total_idle,
                idle_timeout_secs: stats.config.idle_timeout_secs,
                connect_timeout_ms: stats.config.connect_timeout_ms,
            },
            destinations_breakdown,
        }
    }
}

fn format_system_time(time: Option<SystemTime>) -> Option<String> {
    time.map(|ts| DateTime::<Utc>::from(ts).to_rfc3339())
}

// ============================================================================
// System Resources API Types
// ============================================================================

/// System and process resource usage response
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemResourcesResponse {
    /// System-wide CPU usage percentage (0.0 - 100.0)
    pub system_cpu_percent: f32,
    /// System-wide RAM usage percentage (0.0 - 100.0)
    pub system_ram_percent: f32,
    /// System total RAM in bytes
    pub system_ram_total_bytes: u64,
    /// System used RAM in bytes
    pub system_ram_used_bytes: u64,
    /// RustSocks process CPU usage percentage (0.0 - 100.0)
    pub process_cpu_percent: f32,
    /// RustSocks process RAM usage in bytes
    pub process_ram_bytes: u64,
    /// System load average (1 minute)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_average_1m: Option<f64>,
}
