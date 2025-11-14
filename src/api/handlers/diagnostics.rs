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
