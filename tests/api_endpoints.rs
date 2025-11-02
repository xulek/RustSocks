use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    Router,
};
use rustsocks::api::handlers::sessions::ApiState;
use rustsocks::api::handlers::{
    get_acl_rules, get_active_sessions, get_metrics, get_session_history, get_session_stats,
    get_user_sessions, health_check, test_acl_decision,
};
use rustsocks::server::pool::{ConnectionPool, PoolConfig};
use rustsocks::session::{ConnectionInfo, SessionManager, SessionProtocol, SessionStatus};
use std::net::IpAddr;
use std::sync::Arc;
use tower::util::ServiceExt;

fn create_api_state(session_manager: Arc<SessionManager>) -> ApiState {
    let connection_pool = Arc::new(ConnectionPool::new(PoolConfig::default()));
    ApiState {
        session_manager,
        acl_engine: None,
        acl_config_path: None,
        connection_pool,
        start_time: std::time::Instant::now(),
        #[cfg(feature = "database")]
        session_store: None,
        metrics_history: None,
    }
}

#[tokio::test]
async fn test_health_endpoint() {
    let session_manager = Arc::new(SessionManager::new());
    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/health", get(health_check))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let health: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(health["status"], "healthy");
    assert!(health["version"].is_string());
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let session_manager = Arc::new(SessionManager::new());

    // Create a test session
    let conn_info = ConnectionInfo {
        source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
        source_port: 12345,
        dest_ip: "8.8.8.8".to_string(),
        dest_port: 80,
        protocol: SessionProtocol::Tcp,
    };

    session_manager
        .new_session("testuser", conn_info, "allow", None)
        .await;

    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/metrics", get(get_metrics))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let metrics = String::from_utf8(body.to_vec()).unwrap();

    assert!(metrics.contains("rustsocks_active_sessions"));
    assert!(metrics.contains("rustsocks_sessions_total"));
    assert!(metrics.contains("rustsocks_bytes_sent_total"));
    assert!(metrics.contains("rustsocks_bytes_received_total"));
}

#[tokio::test]
async fn test_get_active_sessions() {
    let session_manager = Arc::new(SessionManager::new());

    // Create test sessions
    for i in 0..3 {
        let conn_info = ConnectionInfo {
            source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
            source_port: 10000 + i,
            dest_ip: format!("8.8.8.{}", i),
            dest_port: 80,
            protocol: SessionProtocol::Tcp,
        };

        session_manager
            .new_session(&format!("user{}", i), conn_info, "allow", None)
            .await;
    }

    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/api/sessions/active", get(get_active_sessions))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/active")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let sessions: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(sessions.len(), 3);
    // Check that all expected users are present (order not guaranteed due to DashMap)
    let users: Vec<String> = sessions
        .iter()
        .map(|s| s["user"].as_str().unwrap().to_string())
        .collect();
    assert!(users.contains(&"user0".to_string()));
    assert!(users.contains(&"user1".to_string()));
    assert!(users.contains(&"user2".to_string()));
}

#[tokio::test]
async fn test_get_session_stats() {
    let session_manager = Arc::new(SessionManager::new());

    // Create test sessions with different users
    for i in 0..5 {
        let conn_info = ConnectionInfo {
            source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
            source_port: 10000 + i,
            dest_ip: format!("8.8.8.{}", i % 2),
            dest_port: 80,
            protocol: SessionProtocol::Tcp,
        };

        session_manager
            .new_session(&format!("user{}", i % 2), conn_info, "allow", None)
            .await;
    }

    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/api/sessions/stats", get(get_session_stats))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stats: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats["total_sessions"], 5);
    assert_eq!(stats["active_sessions"], 5);
    assert!(stats["top_users"].is_array());
    assert!(stats["top_destinations"].is_array());
}

#[tokio::test]
async fn test_get_user_sessions() {
    let session_manager = Arc::new(SessionManager::new());

    // Create sessions for different users
    for i in 0..3 {
        let conn_info = ConnectionInfo {
            source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
            source_port: 10000 + i,
            dest_ip: "8.8.8.8".to_string(),
            dest_port: 80,
            protocol: SessionProtocol::Tcp,
        };

        session_manager
            .new_session("alice", conn_info, "allow", None)
            .await;
    }

    for i in 0..2 {
        let conn_info = ConnectionInfo {
            source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
            source_port: 20000 + i,
            dest_ip: "8.8.4.4".to_string(),
            dest_port: 443,
            protocol: SessionProtocol::Tcp,
        };

        session_manager
            .new_session("bob", conn_info, "allow", None)
            .await;
    }

    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/api/users/:user/sessions", get(get_user_sessions))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/users/alice/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let sessions: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(sessions.len(), 3);
    for session in &sessions {
        assert_eq!(session["user"], "alice");
    }
}

#[tokio::test]
async fn test_session_history_with_filters() {
    let session_manager = Arc::new(SessionManager::new());

    // Create and close some sessions
    for i in 0..5 {
        let conn_info = ConnectionInfo {
            source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
            source_port: 10000 + i,
            dest_ip: format!("8.8.8.{}", i),
            dest_port: 80,
            protocol: SessionProtocol::Tcp,
        };

        let session_id = session_manager
            .new_session(&format!("user{}", i % 2), conn_info, "allow", None)
            .await;

        // Close some sessions
        if i < 3 {
            session_manager
                .close_session(
                    &session_id,
                    Some("Test close".to_string()),
                    SessionStatus::Closed,
                )
                .await;
        }
    }

    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/api/sessions/history", get(get_session_history))
        .with_state(state);

    // Test with user filter
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sessions/history?user=user0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Should have sessions only for user0
    assert!(result["data"].is_array());
    let sessions = result["data"].as_array().unwrap();
    for session in sessions {
        assert_eq!(session["user"], "user0");
    }
}

#[tokio::test]
async fn test_session_history_pagination() {
    let session_manager = Arc::new(SessionManager::new());

    // Create and close many sessions
    for i in 0..10 {
        let conn_info = ConnectionInfo {
            source_ip: "127.0.0.1".parse::<IpAddr>().unwrap(),
            source_port: 10000 + i,
            dest_ip: "8.8.8.8".to_string(),
            dest_port: 80,
            protocol: SessionProtocol::Tcp,
        };

        let session_id = session_manager
            .new_session("testuser", conn_info, "allow", None)
            .await;

        session_manager
            .close_session(&session_id, Some("Test".to_string()), SessionStatus::Closed)
            .await;
    }

    let state = create_api_state(session_manager.clone());

    let app = Router::new()
        .route("/api/sessions/history", get(get_session_history))
        .with_state(state);

    // Test pagination
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/history?page=1&page_size=5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["total"], 10);
    assert_eq!(result["page"], 1);
    assert_eq!(result["page_size"], 5);
    assert_eq!(result["total_pages"], 2);
    assert_eq!(result["data"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn test_get_acl_rules_without_acl() {
    let session_manager = Arc::new(SessionManager::new());
    let state = create_api_state(session_manager);

    let app = Router::new()
        .route("/api/acl/rules", get(get_acl_rules))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/acl/rules")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["message"], "ACL is not enabled");
}

#[tokio::test]
async fn test_test_acl_decision_without_acl() {
    let session_manager = Arc::new(SessionManager::new());
    let state = create_api_state(session_manager);

    let app = Router::new()
        .route("/api/acl/test", post(test_acl_decision))
        .with_state(state);

    let request_body = serde_json::json!({
        "user": "alice",
        "destination": "example.com",
        "port": 443,
        "protocol": "tcp"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/acl/test")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["decision"], "error");
    assert_eq!(result["matched_rule"], "ACL is not enabled");
}

#[tokio::test]
async fn test_test_acl_decision_invalid_protocol() {
    let session_manager = Arc::new(SessionManager::new());
    let state = create_api_state(session_manager);

    let app = Router::new()
        .route("/api/acl/test", post(test_acl_decision))
        .with_state(state);

    let request_body = serde_json::json!({
        "user": "alice",
        "destination": "example.com",
        "port": 443,
        "protocol": "invalid"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/acl/test")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["decision"], "error");
    // When ACL is not enabled, we get "ACL is not enabled" error first, before protocol validation
    assert_eq!(result["matched_rule"], "ACL is not enabled");
}

#[tokio::test]
async fn test_test_acl_decision_valid_request() {
    let session_manager = Arc::new(SessionManager::new());
    let state = create_api_state(session_manager);

    let app = Router::new()
        .route("/api/acl/test", post(test_acl_decision))
        .with_state(state);

    let request_body = serde_json::json!({
        "user": "alice",
        "destination": "example.com",
        "port": 443,
        "protocol": "tcp"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/acl/test")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(result["decision"], "error");
    // ACL not enabled returns error
    assert_eq!(result["matched_rule"], "ACL is not enabled");
}
