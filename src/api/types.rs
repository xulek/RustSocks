use serde::{Deserialize, Serialize};

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
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 9090,
            enable_api: false,
            token: None,
        }
    }
}
