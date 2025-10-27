use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde_json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::api::handlers::sessions::ApiState;
use crate::api::handlers::{
    management::{get_acl_rules, get_metrics, health_check, reload_acl, test_acl_decision},
    sessions::{
        get_active_sessions, get_session_detail, get_session_history, get_session_stats,
        get_user_sessions,
    },
};
use crate::api::types::ApiConfig;
use crate::session::SessionManager;
use crate::utils::error::{Result, RustSocksError};

/// Serve Swagger UI HTML
async fn swagger_ui() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>RustSocks API Documentation</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5.11.0/swagger-ui.css" />
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5.11.0/swagger-ui-bundle.js" crossorigin></script>
<script>
  window.onload = () => {
    window.ui = SwaggerUIBundle({
      url: '/openapi.json',
      dom_id: '#swagger-ui',
    });
  };
</script>
</body>
</html>"#,
    )
}

/// OpenAPI spec endpoint
async fn openapi_spec() -> impl IntoResponse {
    (StatusCode::OK, Json(get_openapi_spec()))
}

/// Generate OpenAPI specification
fn get_openapi_spec() -> serde_json::Value {
    serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "RustSocks API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Complete REST API for RustSocks SOCKS5 proxy server with session tracking, ACL management, and metrics",
            "contact": {
                "name": "RustSocks"
            }
        },
        "servers": [
            {
                "url": "http://localhost:9090",
                "description": "Development server"
            }
        ],
        "tags": [
            {
                "name": "Health",
                "description": "Server health checks"
            },
            {
                "name": "Metrics",
                "description": "Prometheus metrics and monitoring"
            },
            {
                "name": "Sessions",
                "description": "Session management and tracking"
            },
            {
                "name": "ACL",
                "description": "Access Control List management"
            },
            {
                "name": "Admin",
                "description": "Administrative operations"
            }
        ],
        "paths": {
            "/health": {
                "get": {
                    "summary": "Health check",
                    "description": "Check if API server is healthy and operational",
                    "tags": ["Health"],
                    "operationId": "healthCheck",
                    "responses": {
                        "200": {
                            "description": "Server is healthy",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "status": {"type": "string", "example": "healthy"},
                                            "version": {"type": "string", "example": "0.1.0"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/metrics": {
                "get": {
                    "summary": "Prometheus metrics",
                    "description": "Get metrics in Prometheus text format",
                    "tags": ["Metrics"],
                    "operationId": "getMetrics",
                    "responses": {
                        "200": {
                            "description": "Prometheus metrics",
                            "content": {
                                "text/plain": {
                                    "schema": {"type": "string"}
                                }
                            }
                        }
                    }
                }
            },
            "/api/sessions/active": {
                "get": {
                    "summary": "Get active sessions",
                    "description": "List all currently active SOCKS5 sessions",
                    "tags": ["Sessions"],
                    "operationId": "getActiveSessions",
                    "responses": {
                        "200": {
                            "description": "List of active sessions",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": {"type": "object"}
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/sessions/history": {
                "get": {
                    "summary": "Get session history",
                    "description": "Get historical session data with optional filtering by user, time, or destination",
                    "tags": ["Sessions"],
                    "operationId": "getSessionHistory",
                    "parameters": [
                        {
                            "name": "user",
                            "in": "query",
                            "schema": {"type": "string"},
                            "description": "Filter by username"
                        },
                        {
                            "name": "hours",
                            "in": "query",
                            "schema": {"type": "integer"},
                            "description": "Filter by time range in hours"
                        },
                        {
                            "name": "dest_ip",
                            "in": "query",
                            "schema": {"type": "string"},
                            "description": "Filter by destination IP"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Session history data",
                            "content": {
                                "application/json": {
                                    "schema": {"type": "object"}
                                }
                            }
                        }
                    }
                }
            },
            "/api/sessions/stats": {
                "get": {
                    "summary": "Get session statistics",
                    "description": "Get aggregated session statistics for monitoring and analytics",
                    "tags": ["Sessions"],
                    "operationId": "getSessionStats",
                    "responses": {
                        "200": {
                            "description": "Aggregated session statistics",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "active_sessions": {"type": "integer"},
                                            "total_sessions": {"type": "integer"},
                                            "total_bytes": {"type": "integer"},
                                            "top_users": {"type": "array"},
                                            "top_destinations": {"type": "array"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/sessions/{id}": {
                "get": {
                    "summary": "Get session detail",
                    "description": "Get detailed information about a specific session",
                    "tags": ["Sessions"],
                    "operationId": "getSessionDetail",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Session ID"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Session details",
                            "content": {
                                "application/json": {
                                    "schema": {"type": "object"}
                                }
                            }
                        },
                        "404": {
                            "description": "Session not found"
                        }
                    }
                }
            },
            "/api/users/{user}/sessions": {
                "get": {
                    "summary": "Get user sessions",
                    "description": "Get all sessions for a specific user",
                    "tags": ["Sessions"],
                    "operationId": "getUserSessions",
                    "parameters": [
                        {
                            "name": "user",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Username"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "User's sessions",
                            "content": {
                                "application/json": {
                                    "schema": {"type": "array"}
                                }
                            }
                        }
                    }
                }
            },
            "/api/acl/rules": {
                "get": {
                    "summary": "Get ACL rules",
                    "description": "Get current Access Control List rules configuration",
                    "tags": ["ACL"],
                    "operationId": "getAclRules",
                    "responses": {
                        "200": {
                            "description": "ACL rules summary",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "user_count": {"type": "integer"},
                                            "group_count": {"type": "integer"},
                                            "message": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "ACL is not enabled"
                        }
                    }
                }
            },
            "/api/acl/test": {
                "post": {
                    "summary": "Test ACL decision",
                    "description": "Test if a connection would be allowed or blocked by ACL rules",
                    "tags": ["ACL"],
                    "operationId": "testAclDecision",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "user": {"type": "string", "example": "alice"},
                                        "destination": {"type": "string", "example": "192.168.1.1"},
                                        "port": {"type": "integer", "example": 443},
                                        "protocol": {"type": "string", "enum": ["tcp", "udp", "both"], "example": "tcp"}
                                    },
                                    "required": ["user", "destination", "port", "protocol"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "ACL decision result",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "decision": {"type": "string", "enum": ["allow", "block"]},
                                            "matched_rule": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "Invalid parameters or ACL not enabled"
                        }
                    }
                }
            },
            "/api/admin/reload-acl": {
                "post": {
                    "summary": "Reload ACL configuration",
                    "description": "Reload ACL rules from configuration file without restarting server",
                    "tags": ["Admin"],
                    "operationId": "reloadAcl",
                    "responses": {
                        "200": {
                            "description": "ACL reloaded successfully",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "success": {"type": "boolean"},
                                            "message": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "ACL is not enabled"
                        },
                        "500": {
                            "description": "Failed to reload ACL configuration"
                        }
                    }
                }
            }
        }
    })
}

/// Start the REST API server with Swagger documentation
pub async fn start_api_server(
    config: ApiConfig,
    session_manager: Arc<SessionManager>,
    acl_engine: Option<Arc<crate::acl::AclEngine>>,
    acl_config_path: Option<String>,
) -> Result<JoinHandle<()>> {
    if !config.enable_api {
        info!("API server disabled");
        return Err(RustSocksError::Config("API server disabled".to_string()));
    }

    let state = ApiState {
        session_manager,
        acl_engine,
        acl_config_path,
    };

    // Build router with all endpoints and Swagger
    let app = Router::new()
        // Swagger UI
        .route("/swagger-ui/", get(swagger_ui))
        .route("/openapi.json", get(openapi_spec))
        // Health and metrics
        .route("/health", get(health_check))
        .route("/metrics", get(get_metrics))
        // Session endpoints
        .route("/api/sessions/active", get(get_active_sessions))
        .route("/api/sessions/history", get(get_session_history))
        .route("/api/sessions/stats", get(get_session_stats))
        .route("/api/sessions/:id", get(get_session_detail))
        .route("/api/users/:user/sessions", get(get_user_sessions))
        // Management endpoints
        .route("/api/admin/reload-acl", post(reload_acl))
        .route("/api/acl/rules", get(get_acl_rules))
        .route("/api/acl/test", post(test_acl_decision))
        // Layer with state and body limit
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB max body
        .with_state(state);

    // Bind and listen
    let addr: SocketAddr = format!("{}:{}", config.bind_address, config.bind_port)
        .parse()
        .map_err(|e| RustSocksError::Config(format!("Invalid bind address: {}", e)))?;

    let listener = TcpListener::bind(&addr).await?;
    info!("API server listening on http://{}", addr);
    info!("Swagger UI available at http://{}/swagger-ui/", addr);

    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, app);
        if let Err(err) = server.await {
            error!("API server error: {}", err);
        }
    });

    Ok(handle)
}
