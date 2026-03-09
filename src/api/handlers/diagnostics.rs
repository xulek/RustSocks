use axum::{extract::State, http::StatusCode, Json};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::{timeout, Instant};

use crate::api::handlers::sessions::ApiState;
use crate::api::types::{ConnectivityTestRequest, ConnectivityTestResponse};

/// POST /api/diagnostics/connectivity - test TCP connectivity to a destination
pub async fn test_tcp_connectivity(
    State(_state): State<ApiState>,
    Json(request): Json<ConnectivityTestRequest>,
) -> (StatusCode, Json<ConnectivityTestResponse>) {
    let address = request.address.trim().to_string();
    let port = request.port;
    let timeout_ms = request.timeout_ms.unwrap_or(3000).clamp(1, 120_000);

    if address.is_empty() {
        let response = ConnectivityTestResponse {
            address,
            port,
            success: false,
            latency_ms: None,
            message: "Destination address cannot be empty".to_string(),
            error: Some("empty_address".to_string()),
        };
        return (StatusCode::BAD_REQUEST, Json(response));
    }

    if port == 0 {
        let response = ConnectivityTestResponse {
            address,
            port,
            success: false,
            latency_ms: None,
            message: "Port must be between 1 and 65535".to_string(),
            error: Some("invalid_port".to_string()),
        };
        return (StatusCode::BAD_REQUEST, Json(response));
    }

    let ip_addr: IpAddr = match address.parse() {
        Ok(ip) => ip,
        Err(_) => {
            let response = ConnectivityTestResponse {
                address,
                port,
                success: false,
                latency_ms: None,
                message: "Invalid IP address format".to_string(),
                error: Some("invalid_ip".to_string()),
            };
            return (StatusCode::BAD_REQUEST, Json(response));
        }
    };

    let socket_addr = SocketAddr::new(ip_addr, port);
    let timeout_duration = Duration::from_millis(timeout_ms);
    let started = Instant::now();

    match timeout(timeout_duration, TcpStream::connect(socket_addr)).await {
        Ok(Ok(stream)) => {
            let latency = started.elapsed().as_millis();
            drop(stream);
            let response = ConnectivityTestResponse {
                address,
                port,
                success: true,
                latency_ms: Some(latency as u64),
                message: "Connection successful".to_string(),
                error: None,
            };
            (StatusCode::OK, Json(response))
        }
        Ok(Err(err)) => {
            let response = ConnectivityTestResponse {
                address,
                port,
                success: false,
                latency_ms: Some(started.elapsed().as_millis() as u64),
                message: format!("Connection failed: {}", err),
                error: Some(err.to_string()),
            };
            (StatusCode::OK, Json(response))
        }
        Err(_) => {
            let response = ConnectivityTestResponse {
                address,
                port,
                success: false,
                latency_ms: Some(timeout_ms),
                message: "Connection attempt timed out".to_string(),
                error: Some("timeout".to_string()),
            };
            (StatusCode::OK, Json(response))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::server::pool::{ConnectionPool, PoolConfig};
    use crate::session::{MetricsHistory, SessionManager};
    use std::sync::Arc;

    fn test_state() -> ApiState {
        ApiState {
            session_manager: Arc::new(SessionManager::new()),
            acl_engine: None,
            acl_config_path: None,
            connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
            start_time: std::time::Instant::now(),
            #[cfg(feature = "database")]
            session_store: None,
            metrics_history: Some(Arc::new(MetricsHistory::new(4, 1))),
            telemetry_history: None,
            config_path: None,
            config_snapshot: Arc::new(Config::default()),
            original_args: Arc::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn rejects_empty_address() {
        let (status, Json(response)) = test_tcp_connectivity(
            State(test_state()),
            Json(ConnectivityTestRequest {
                address: "   ".to_string(),
                port: 80,
                timeout_ms: None,
            }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response.error.as_deref(), Some("empty_address"));
    }

    #[tokio::test]
    async fn rejects_invalid_ip_and_port() {
        let (status, Json(response)) = test_tcp_connectivity(
            State(test_state()),
            Json(ConnectivityTestRequest {
                address: "not-an-ip".to_string(),
                port: 8080,
                timeout_ms: Some(50),
            }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response.error.as_deref(), Some("invalid_ip"));

        let (status, Json(response)) = test_tcp_connectivity(
            State(test_state()),
            Json(ConnectivityTestRequest {
                address: "127.0.0.1".to_string(),
                port: 0,
                timeout_ms: Some(50),
            }),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response.error.as_deref(), Some("invalid_port"));
    }

    #[tokio::test]
    async fn returns_success_for_reachable_listener() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let addr = listener.local_addr().expect("local addr");

        let accept_task = tokio::spawn(async move {
            let _ = listener.accept().await.expect("accept");
        });

        let (status, Json(response)) = test_tcp_connectivity(
            State(test_state()),
            Json(ConnectivityTestRequest {
                address: "127.0.0.1".to_string(),
                port: addr.port(),
                timeout_ms: Some(500),
            }),
        )
        .await;

        accept_task.await.expect("accept task");

        assert_eq!(status, StatusCode::OK);
        assert!(response.success);
        assert!(response.latency_ms.is_some());
        assert!(response.error.is_none());
    }
}
