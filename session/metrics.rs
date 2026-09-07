use lazy_static::lazy_static;
use prometheus::{
    register_histogram, register_int_counter, register_int_gauge, Histogram, HistogramOpts,
    IntCounter, IntGauge,
};

lazy_static! {
    pub static ref ACTIVE_SESSIONS: IntGauge = register_int_gauge!(
        "rustsocks_active_sessions",
        "Number of currently active SOCKS5 sessions"
    )
    .expect("register rustsocks_active_sessions gauge");
    pub static ref TOTAL_SESSIONS: IntCounter = register_int_counter!(
        "rustsocks_sessions_total",
        "Total number of accepted SOCKS5 sessions since start"
    )
    .expect("register rustsocks_sessions_total counter");
    pub static ref REJECTED_SESSIONS: IntCounter = register_int_counter!(
        "rustsocks_sessions_rejected_total",
        "Total number of rejected SOCKS5 sessions (e.g. ACL)"
    )
    .expect("register rustsocks_sessions_rejected_total counter");
    pub static ref SESSION_DURATION: Histogram = register_histogram!(HistogramOpts::new(
        "rustsocks_session_duration_seconds",
        "Observed SOCKS5 session duration in seconds"
    )
    .buckets(vec![
        0.5, 1.0, 5.0, 15.0, 60.0, 300.0, 900.0, 1800.0, 3600.0, 7200.0
    ]))
    .expect("register rustsocks_session_duration_seconds histogram");
    pub static ref TOTAL_BYTES_SENT: IntCounter = register_int_counter!(
        "rustsocks_bytes_sent_total",
        "Total bytes sent from client to upstream across all sessions"
    )
    .expect("register rustsocks_bytes_sent_total counter");
    pub static ref TOTAL_BYTES_RECEIVED: IntCounter = register_int_counter!(
        "rustsocks_bytes_received_total",
        "Total bytes received from upstream to client across all sessions"
    )
    .expect("register rustsocks_bytes_received_total counter");
}

#[derive(Debug, Clone, Copy)]
pub struct SessionMetrics;

impl SessionMetrics {
    /// Ensure every RustSocks metric is registered before a Prometheus gather.
    ///
    /// Metrics are lazily initialized, so counters that have never been updated would
    /// otherwise be missing entirely from `/metrics` instead of being exported as zero.
    #[inline]
    pub fn ensure_registered() {
        lazy_static::initialize(&ACTIVE_SESSIONS);
        lazy_static::initialize(&TOTAL_SESSIONS);
        lazy_static::initialize(&REJECTED_SESSIONS);
        lazy_static::initialize(&SESSION_DURATION);
        lazy_static::initialize(&TOTAL_BYTES_SENT);
        lazy_static::initialize(&TOTAL_BYTES_RECEIVED);
    }

    #[inline]
    pub fn record_session_start(_user: &str) {
        ACTIVE_SESSIONS.inc();
        TOTAL_SESSIONS.inc();
    }

    #[inline]
    pub fn record_session_close(duration_secs: Option<u64>) {
        ACTIVE_SESSIONS.dec();
        if let Some(duration) = duration_secs {
            SESSION_DURATION.observe(duration as f64);
        }
    }

    #[inline]
    pub fn record_rejected_session(_user: &str) {
        REJECTED_SESSIONS.inc();
    }

    #[inline]
    pub fn record_traffic(_user: &str, bytes_sent: u64, bytes_received: u64) {
        if bytes_sent > 0 {
            TOTAL_BYTES_SENT.inc_by(bytes_sent);
        }

        if bytes_received > 0 {
            TOTAL_BYTES_RECEIVED.inc_by(bytes_received);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SessionMetrics;

    #[test]
    fn ensure_registered_exports_zero_traffic_counters() {
        use prometheus::{Encoder, TextEncoder};

        SessionMetrics::ensure_registered();
        let encoder = TextEncoder::new();
        let mut buffer = Vec::new();
        encoder
            .encode(&prometheus::gather(), &mut buffer)
            .expect("encode Prometheus metrics");
        let output = String::from_utf8(buffer).expect("metrics output is UTF-8");

        assert!(output.contains("rustsocks_bytes_sent_total"));
        assert!(output.contains("rustsocks_bytes_received_total"));
    }
}
