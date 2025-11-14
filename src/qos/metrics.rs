use lazy_static::lazy_static;
use prometheus::{
    register_histogram, register_int_counter_vec, register_int_gauge, Histogram, IntCounterVec,
    IntGauge,
};

lazy_static! {
    pub static ref ACTIVE_QOS_USERS: IntGauge = register_int_gauge!(
        "rustsocks_qos_active_users",
        "Number of users with at least one active QoS-managed connection"
    )
    .expect("register rustsocks_qos_active_users gauge");
    pub static ref BANDWIDTH_ALLOCATED: IntCounterVec = register_int_counter_vec!(
        "rustsocks_qos_bandwidth_allocated_bytes_total",
        "Total bytes allocated through the QoS engine per user and direction",
        &["user", "direction"]
    )
    .expect("register rustsocks_qos_bandwidth_allocated_bytes_total counter vec");
    pub static ref ALLOCATION_WAIT: Histogram = register_histogram!(
        "rustsocks_qos_allocation_wait_seconds",
        "Observed wait time while throttling traffic for QoS allocations"
    )
    .expect("register rustsocks_qos_allocation_wait_seconds histogram");
}

#[derive(Debug, Clone, Copy)]
pub struct QosMetrics;

impl QosMetrics {
    #[inline]
    pub fn user_activated() {
        ACTIVE_QOS_USERS.inc();
    }

    #[inline]
    pub fn user_deactivated() {
        ACTIVE_QOS_USERS.dec();
    }

    #[inline]
    pub fn record_allocation(user: &str, direction: &str, bytes: u64) {
        BANDWIDTH_ALLOCATED
            .with_label_values(&[user, direction])
            .inc_by(bytes);
    }

    #[inline]
    pub fn observe_wait(duration_secs: f64) {
        ALLOCATION_WAIT.observe(duration_secs);
    }
}

#[inline]
pub fn init() {
    lazy_static::initialize(&ACTIVE_QOS_USERS);
    lazy_static::initialize(&BANDWIDTH_ALLOCATED);
    lazy_static::initialize(&ALLOCATION_WAIT);
}
