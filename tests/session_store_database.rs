#![cfg(feature = "database")]

use chrono::{Duration as ChronoDuration, Utc};
use rustsocks::session::{
    ConnectionInfo, Session, SessionFilter, SessionProtocol, SessionStatus, SessionStore,
};
use std::net::{IpAddr, Ipv4Addr};
use tempfile::TempDir;
use uuid::Uuid;

async fn setup_store() -> (SessionStore, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("sessions.db");
    let db_url = format!("sqlite://{}", db_path.display());
    let store = SessionStore::connect(&db_url).await.unwrap();
    (store, temp_dir)
}

fn build_session(
    user: &str,
    dest_ip: &str,
    status: SessionStatus,
    start_time: chrono::DateTime<Utc>,
    duration_secs: Option<u64>,
    bytes_sent: u64,
    bytes_received: u64,
    close_reason: Option<&str>,
) -> Session {
    let connection = ConnectionInfo {
        source_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
        source_port: 5000,
        dest_ip: dest_ip.to_string(),
        dest_port: 443,
        protocol: SessionProtocol::Tcp,
    };

    let mut session = Session::new(user, connection, "allow", Some("Allow rule".to_string()));
    session.start_time = start_time;
    session.bytes_sent = bytes_sent;
    session.bytes_received = bytes_received;
    session.status = status;
    session.close_reason = close_reason.map(|reason| reason.to_string());

    if let Some(secs) = duration_secs {
        session.duration_secs = Some(secs);
        session.end_time = Some(start_time + ChronoDuration::seconds(secs as i64));
    } else {
        session.duration_secs = None;
        session.end_time = None;
    }

    session
}

#[tokio::test]
async fn query_filters_and_sorting() {
    let (store, _temp_dir) = setup_store().await;
    let now = Utc::now();

    let session1 = build_session(
        "alice",
        "example.com",
        SessionStatus::Closed,
        now - ChronoDuration::hours(2),
        Some(300),
        3000,
        1200,
        Some("Done"),
    );
    let session2 = build_session(
        "alice",
        "example.com",
        SessionStatus::Closed,
        now - ChronoDuration::hours(1),
        Some(30),
        500,
        100,
        Some("Done"),
    );
    let session3 = build_session(
        "bob",
        "internal.local",
        SessionStatus::Active,
        now - ChronoDuration::minutes(30),
        None,
        200,
        100,
        None,
    );

    store.insert_session(&session1).await.unwrap();
    store.insert_session(&session2).await.unwrap();
    store.insert_session(&session3).await.unwrap();

    let mut filtered = SessionFilter::default();
    filtered.user = Some("alice".to_string());
    filtered.status = Some(SessionStatus::Closed);
    filtered.dest_ip = Some("example.com".to_string());
    filtered.min_duration_secs = Some(60);
    filtered.min_bytes = Some(4000);
    filtered.sort_by = Some("bytes_sent".to_string());
    filtered.sort_dir = Some("asc".to_string());
    filtered.limit = None;

    let results = store.query_sessions(&filtered).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].session_id, session1.session_id);

    let mut sorted = SessionFilter::default();
    sorted.user = Some("alice".to_string());
    sorted.status = Some(SessionStatus::Closed);
    sorted.sort_by = Some("bytes_sent".to_string());
    sorted.sort_dir = Some("asc".to_string());
    sorted.limit = None;

    let results = store.query_sessions(&sorted).await.unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].session_id, session2.session_id);
    assert_eq!(results[1].session_id, session1.session_id);
}

#[tokio::test]
async fn count_ids_and_get_session() {
    let (store, _temp_dir) = setup_store().await;
    let now = Utc::now();

    let session1 = build_session(
        "alice",
        "example.com",
        SessionStatus::Closed,
        now - ChronoDuration::minutes(10),
        Some(20),
        100,
        50,
        Some("Done"),
    );
    let session2 = build_session(
        "bob",
        "internal.local",
        SessionStatus::Active,
        now - ChronoDuration::minutes(5),
        None,
        75,
        25,
        None,
    );

    store.insert_session(&session1).await.unwrap();
    store.insert_session(&session2).await.unwrap();

    let mut filter = SessionFilter::default();
    filter.user = Some("alice".to_string());
    filter.limit = None;
    let count = store.count_sessions(&filter).await.unwrap();
    assert_eq!(count, 1);

    let approx = store.approximate_total_sessions().await.unwrap();
    assert!(approx >= 2);

    let ids = store
        .existing_session_ids(&[session1.session_id, Uuid::new_v4()])
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&session1.session_id));

    let fetched = store
        .get_session(&session1.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.user.as_ref(), "alice");

    let missing = store.get_session(&Uuid::new_v4()).await.unwrap();
    assert!(missing.is_none());
}

#[tokio::test]
async fn close_all_active_sessions_marks_closed() {
    let (store, _temp_dir) = setup_store().await;
    let now = Utc::now();

    let active = build_session(
        "alice",
        "example.com",
        SessionStatus::Active,
        now - ChronoDuration::minutes(10),
        None,
        120,
        60,
        None,
    );
    let closed = build_session(
        "bob",
        "example.com",
        SessionStatus::Closed,
        now - ChronoDuration::minutes(20),
        Some(60),
        200,
        100,
        Some("Finished"),
    );

    store.insert_session(&active).await.unwrap();
    store.insert_session(&closed).await.unwrap();

    let affected = store.close_all_active_sessions().await.unwrap();
    assert_eq!(affected, 1);

    let updated = store
        .get_session(&active.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, SessionStatus::Closed);
    assert_eq!(updated.close_reason.as_deref(), Some("Server restart"));
    assert!(updated.end_time.is_some());
    assert!(updated.duration_secs.is_some());

    let unchanged = store
        .get_session(&closed.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.status, SessionStatus::Closed);
    assert_eq!(unchanged.close_reason.as_deref(), Some("Finished"));
}

#[tokio::test]
async fn cleanup_older_than_removes_sessions() {
    let (store, _temp_dir) = setup_store().await;
    let now = Utc::now();

    let old = build_session(
        "alice",
        "example.com",
        SessionStatus::Closed,
        now - ChronoDuration::days(3),
        Some(120),
        100,
        50,
        Some("Old"),
    );
    let recent = build_session(
        "alice",
        "example.com",
        SessionStatus::Closed,
        now - ChronoDuration::hours(6),
        Some(60),
        100,
        50,
        Some("Recent"),
    );

    store.insert_session(&old).await.unwrap();
    store.insert_session(&recent).await.unwrap();

    let affected = store.cleanup_older_than(1).await.unwrap();
    assert_eq!(affected, 1);

    let mut filter = SessionFilter::default();
    filter.limit = None;
    let results = store.query_sessions(&filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].session_id, recent.session_id);
}

#[tokio::test]
async fn metrics_insert_query_and_cleanup() {
    let (store, _temp_dir) = setup_store().await;
    let now = Utc::now();

    let older = now - ChronoDuration::hours(2);
    let recent = now - ChronoDuration::minutes(30);

    store.insert_metric(&older, 5, 10, 100).await.unwrap();
    store.insert_metric(&recent, 7, 12, 200).await.unwrap();

    let start = now - ChronoDuration::hours(1);
    let metrics = store.query_metrics(Some(&start), None).await.unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].bandwidth, 200);

    let limited = store.query_metrics(None, Some(1)).await.unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].bandwidth, 200);

    let removed = store.cleanup_old_metrics(1).await.unwrap();
    assert_eq!(removed, 1);

    let remaining = store.query_metrics(None, None).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].bandwidth, 200);
}
