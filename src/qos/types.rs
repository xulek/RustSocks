use serde::{Deserialize, Serialize};

/// QoS configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QosConfig {
    /// Enable QoS
    pub enabled: bool,

    /// QoS algorithm ("htb" or "simple")
    #[serde(default = "default_algorithm")]
    pub algorithm: String,

    /// HTB-specific configuration
    #[serde(default)]
    pub htb: HtbConfig,

    /// Connection limits
    #[serde(default)]
    pub connection_limits: ConnectionLimits,
}

fn default_algorithm() -> String {
    "htb".to_string()
}

impl Default for QosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            algorithm: "htb".to_string(),
            htb: HtbConfig::default(),
            connection_limits: ConnectionLimits::default(),
        }
    }
}

/// Hierarchical Token Bucket configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HtbConfig {
    /// Global bandwidth limit in bytes per second
    /// Example: 125_000_000 = 1 Gbps
    #[serde(default = "default_global_bandwidth")]
    pub global_bandwidth_bytes_per_sec: u64,

    /// Guaranteed minimum bandwidth per user in bytes per second
    /// Example: 131_072 = 1 Mbps
    #[serde(default = "default_guaranteed_bandwidth")]
    pub guaranteed_bandwidth_bytes_per_sec: u64,

    /// Maximum bandwidth per user in bytes per second (when borrowing)
    /// Example: 12_500_000 = 100 Mbps
    #[serde(default = "default_max_bandwidth")]
    pub max_bandwidth_bytes_per_sec: u64,

    /// Burst size in bytes (how much can be consumed instantly)
    /// Example: 1_048_576 = 1 MB
    #[serde(default = "default_burst_size")]
    pub burst_size_bytes: u64,

    /// How often to refill token buckets (milliseconds)
    #[serde(default = "default_refill_interval")]
    pub refill_interval_ms: u64,

    /// Enable fair sharing between active users
    #[serde(default = "default_fair_sharing")]
    pub fair_sharing_enabled: bool,

    /// How often to recalculate fair shares (milliseconds)
    #[serde(default = "default_rebalance_interval")]
    pub rebalance_interval_ms: u64,

    /// Inactivity threshold - user is considered idle after this many seconds without traffic
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
}

fn default_global_bandwidth() -> u64 {
    125_000_000 // 1 Gbps
}

fn default_guaranteed_bandwidth() -> u64 {
    131_072 // 1 Mbps
}

fn default_max_bandwidth() -> u64 {
    12_500_000 // 100 Mbps
}

fn default_burst_size() -> u64 {
    1_048_576 // 1 MB
}

fn default_refill_interval() -> u64 {
    50 // 50ms
}

fn default_fair_sharing() -> bool {
    true
}

fn default_rebalance_interval() -> u64 {
    100 // 100ms
}

fn default_idle_timeout() -> u64 {
    5 // 5 seconds
}

impl Default for HtbConfig {
    fn default() -> Self {
        Self {
            global_bandwidth_bytes_per_sec: default_global_bandwidth(),
            guaranteed_bandwidth_bytes_per_sec: default_guaranteed_bandwidth(),
            max_bandwidth_bytes_per_sec: default_max_bandwidth(),
            burst_size_bytes: default_burst_size(),
            refill_interval_ms: default_refill_interval(),
            fair_sharing_enabled: default_fair_sharing(),
            rebalance_interval_ms: default_rebalance_interval(),
            idle_timeout_secs: default_idle_timeout(),
        }
    }
}

/// Connection limit configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionLimits {
    /// Maximum connections per user
    #[serde(default = "default_max_connections_per_user")]
    pub max_connections_per_user: usize,

    /// Maximum total connections (global)
    #[serde(default = "default_max_connections_global")]
    pub max_connections_global: usize,
}

fn default_max_connections_per_user() -> usize {
    20
}

fn default_max_connections_global() -> usize {
    10_000
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            max_connections_per_user: default_max_connections_per_user(),
            max_connections_global: default_max_connections_global(),
        }
    }
}

/// User bandwidth allocation info
#[derive(Debug, Clone)]
pub struct UserAllocation {
    /// Username
    pub user: String,

    /// Current allocated bandwidth (bytes/sec)
    pub allocated_bandwidth: u64,

    /// Guaranteed bandwidth (bytes/sec)
    pub guaranteed_bandwidth: u64,

    /// Maximum possible bandwidth (bytes/sec)
    pub max_bandwidth: u64,

    /// Current demand (estimated from recent usage)
    pub current_demand: u64,

    /// Is user currently active?
    pub is_active: bool,

    /// Active connections count
    pub active_connections: usize,
}
