use lazy_static::lazy_static;
use prometheus::{
    register_histogram, register_int_counter, register_int_counter_vec, register_int_gauge,
    Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge,
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
    pub static ref USER_SESSIONS: IntCounterVec = register_int_counter_vec!(
        "rustsocks_user_sessions_total",
        "Total sessions attributed to each authenticated user",
        &["user"]
    )
    .expect("register rustsocks_user_sessions_total counter_vec");
    pub static ref USER_BANDWIDTH: IntCounterVec = register_int_counter_vec!(
        "rustsocks_user_bandwidth_bytes_total",
        "Total bytes transferred per user and direction",
        &["user", "direction"]
    )
    .expect("register rustsocks_user_bandwidth_bytes_total counter_vec");
}

#[derive(Debug, Clone, Copy)]
pub struct SessionMetrics;

impl SessionMetrics {
    #[inline]
    pub fn record_session_start(user: &str) {
        ACTIVE_SESSIONS.inc();
        TOTAL_SESSIONS.inc();
        USER_SESSIONS.with_label_values(&[user]).inc();
    }

    #[inline]
    pub fn record_session_close(duration_secs: Option<u64>) {
        ACTIVE_SESSIONS.dec();
        if let Some(duration) = duration_secs {
            SESSION_DURATION.observe(duration as f64);
        }
    }

    #[inline]
    pub fn record_rejected_session(user: &str) {
        REJECTED_SESSIONS.inc();
        USER_SESSIONS.with_label_values(&[user]).inc();
    }

    #[inline]
    pub fn record_traffic(user: &str, bytes_sent: u64, bytes_received: u64) {
        if bytes_sent > 0 {
            TOTAL_BYTES_SENT.inc_by(bytes_sent);
            USER_BANDWIDTH
                .with_label_values(&[user, "sent"])
                .inc_by(bytes_sent);
        }

        if bytes_received > 0 {
            TOTAL_BYTES_RECEIVED.inc_by(bytes_received);
            USER_BANDWIDTH
                .with_label_values(&[user, "received"])
                .inc_by(bytes_received);
        }
    }
}
