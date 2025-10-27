mod htb;
mod metrics;
mod token_bucket;
mod types;

pub use htb::HtbQos;
pub use metrics::QosMetrics;
pub use types::{ConnectionLimits, HtbConfig, QosConfig, UserAllocation};

use crate::utils::error::{Result, RustSocksError};
use std::sync::Arc;
use tracing::info;

/// QoS Engine - manages bandwidth allocation and connection limiting
#[derive(Clone)]
pub enum QosEngine {
    /// No QoS (disabled)
    None,

    /// Hierarchical Token Bucket with fair sharing
    Htb(Arc<HtbQos>),
}

impl QosEngine {
    /// Create QoS engine from configuration
    pub async fn from_config(config: QosConfig) -> Result<Self> {
        metrics::init();

        if !config.enabled {
            info!("QoS disabled");
            return Ok(Self::None);
        }

        match config.algorithm.as_str() {
            "htb" => {
                info!(
                    global_bandwidth = config.htb.global_bandwidth_bytes_per_sec,
                    guaranteed_per_user = config.htb.guaranteed_bandwidth_bytes_per_sec,
                    max_per_user = config.htb.max_bandwidth_bytes_per_sec,
                    fair_sharing = config.htb.fair_sharing_enabled,
                    "Initializing HTB QoS engine"
                );

                let htb = HtbQos::new(config.htb);
                htb.start().await;

                Ok(Self::Htb(Arc::new(htb)))
            }
            other => Err(RustSocksError::Config(format!(
                "Unknown QoS algorithm: {}",
                other
            ))),
        }
    }

    /// Allocate bandwidth for user
    pub async fn allocate_bandwidth(&self, user: &str, bytes: u64) -> Result<()> {
        match self {
            Self::None => Ok(()),
            Self::Htb(htb) => htb.allocate_bandwidth(user, bytes).await,
        }
    }

    /// Check connection limit and increment if allowed
    pub fn check_and_inc_connection(&self, user: &str, limits: &ConnectionLimits) -> Result<usize> {
        match self {
            Self::None => Ok(0),
            Self::Htb(htb) => {
                // Check global limit
                let global_count = htb.get_total_connections();
                if global_count >= limits.max_connections_global {
                    return Err(RustSocksError::Config(format!(
                        "Global connection limit reached: {}/{}",
                        global_count, limits.max_connections_global
                    )));
                }

                // Check per-user limit
                let user_count = htb.get_user_connections(user);
                if user_count >= limits.max_connections_per_user {
                    return Err(RustSocksError::Config(format!(
                        "User connection limit reached for '{}': {}/{}",
                        user, user_count, limits.max_connections_per_user
                    )));
                }

                // Increment
                let count = htb.inc_user_connections(user)?;
                if count == 1 {
                    metrics::QosMetrics::user_activated();
                }
                Ok(count)
            }
        }
    }

    /// Decrement user connection count
    pub fn dec_user_connection(&self, user: &str) {
        match self {
            Self::None => {}
            Self::Htb(htb) => {
                let remaining = htb.dec_user_connections(user);
                if remaining == 0 {
                    metrics::QosMetrics::user_deactivated();
                }
            }
        }
    }

    /// Get user connection count
    pub fn get_user_connections(&self, user: &str) -> usize {
        match self {
            Self::None => 0,
            Self::Htb(htb) => htb.get_user_connections(user),
        }
    }

    /// Get total connection count
    pub fn get_total_connections(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Htb(htb) => htb.get_total_connections(),
        }
    }

    /// Get current bandwidth allocations for all users
    pub async fn get_user_allocations(&self) -> Vec<UserAllocation> {
        match self {
            Self::None => Vec::new(),
            Self::Htb(htb) => htb.get_user_allocations().await,
        }
    }

    /// Check if QoS is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::None)
    }
}

impl Drop for QosEngine {
    fn drop(&mut self) {
        // Stop rebalancing task on drop
        if let Self::Htb(htb) = self {
            let htb = htb.clone();
            tokio::spawn(async move {
                htb.stop().await;
            });
        }
    }
}
