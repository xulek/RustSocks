#[cfg(feature = "metrics")]
mod enabled {
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
            "Total bytes allocated through the QoS engine per direction",
            &["direction"]
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
        pub fn record_allocation(_user: &str, direction: &str, bytes: u64) {
            BANDWIDTH_ALLOCATED
                .with_label_values(&[direction])
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

        // Counter vectors only expose concrete series after their label values are
        // instantiated. Register the two supported directions up front so /metrics
        // has a stable schema even before the first proxied byte.
        BANDWIDTH_ALLOCATED
            .with_label_values(&["upload"])
            .inc_by(0);
        BANDWIDTH_ALLOCATED
            .with_label_values(&["download"])
            .inc_by(0);
    }
}

#[cfg(not(feature = "metrics"))]
mod disabled {
    #[derive(Debug, Clone, Copy)]
    pub struct QosMetrics;

    impl QosMetrics {
        #[inline]
        pub fn user_activated() {}

        #[inline]
        pub fn user_deactivated() {}

        #[inline]
        pub fn record_allocation(_user: &str, _direction: &str, _bytes: u64) {}

        #[inline]
        pub fn observe_wait(_duration_secs: f64) {}
    }

    #[inline]
    pub fn init() {}
}

#[cfg(not(feature = "metrics"))]
pub use disabled::*;
#[cfg(feature = "metrics")]
pub use enabled::*;

#[cfg(all(test, feature = "metrics"))]
mod tests {
    use super::enabled::init;
    use prometheus::{Encoder, TextEncoder};

    #[test]
    fn init_exports_stable_direction_series() {
        init();

        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder
            .encode(&prometheus::gather(), &mut buffer)
            .expect("encode Prometheus metrics");
        let output = String::from_utf8(buffer).expect("metrics output is UTF-8");

        assert!(output.contains("rustsocks_qos_active_users"));
        assert!(output.contains(
            "rustsocks_qos_bandwidth_allocated_bytes_total{direction=\"upload\"}"
        ));
        assert!(output.contains(
            "rustsocks_qos_bandwidth_allocated_bytes_total{direction=\"download\"}"
        ));
    }
}
