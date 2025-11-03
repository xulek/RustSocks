use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde_json;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;
use tracing::{error, info, warn};

use crate::api::handlers::sessions::ApiState;
use crate::api::handlers::{
    acl_management::{
        add_group_rule, add_user_rule, create_group, delete_group, delete_group_rule,
        delete_user_rule, get_global_settings, get_group_detail, get_user_detail, list_groups,
        list_users, search_rules, update_global_settings, update_group_rule, update_user_rule,
    },
    get_pool_stats, get_system_resources,
    management::{get_acl_rules, get_metrics, health_check, reload_acl, test_acl_decision},
    sessions::{
        get_active_sessions, get_metrics_history, get_session_detail, get_session_history,
        get_session_stats, get_user_sessions, terminate_session,
    },
    test_tcp_connectivity,
};
use crate::api::types::ApiConfig;
use crate::server::pool::ConnectionPool;
use crate::session::SessionManager;
use crate::utils::error::{Result, RustSocksError};

/// Serve Swagger UI HTML with dynamic base path
async fn swagger_ui(base_path: String) -> Html<String> {
    let openapi_url = if base_path.is_empty() {
        "/openapi.json".to_string()
    } else {
        format!("{}/openapi.json", base_path)
    };

    let html = format!(
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
  window.onload = () => {{
    window.ui = SwaggerUIBundle({{
      url: '{}',
      dom_id: '#swagger-ui',
    }});
  }};
</script>
</body>
</html>"#,
        openapi_url
    );

    Html(html)
}

/// OpenAPI spec endpoint
async fn openapi_spec(base_path: String) -> impl IntoResponse {
    (StatusCode::OK, Json(get_openapi_spec(base_path)))
}

/// Generate OpenAPI specification
fn get_openapi_spec(base_path: String) -> serde_json::Value {
    let server_url = if base_path.is_empty() {
        "http://localhost:9090".to_string()
    } else {
        format!("http://localhost:9090{}", base_path)
    };

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
                "url": server_url,
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
                "description": "Access Control List management (read-only)"
            },
            {
                "name": "ACL-Groups",
                "description": "ACL rule management for groups (LDAP integration)"
            },
            {
                "name": "ACL-Users",
                "description": "ACL rule management for users (per-user overrides)"
            },
            {
                "name": "ACL-Global",
                "description": "Global ACL settings and search"
            },
            {
                "name": "Admin",
                "description": "Administrative operations"
            },
            {
                "name": "Diagnostics",
                "description": "Troubleshooting and connectivity checks"
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
            "/api/diagnostics/connectivity": {
                "post": {
                    "summary": "Test TCP connectivity",
                    "description": "Attempt a TCP connection to the specified IP address and port",
                    "tags": ["Diagnostics"],
                    "operationId": "testTcpConnectivity",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "address": {"type": "string", "example": "8.8.8.8"},
                                        "port": {"type": "integer", "format": "int32", "example": 53},
                                        "timeout_ms": {"type": "integer", "format": "int64", "example": 3000}
                                    },
                                    "required": ["address", "port"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Connectivity test result",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "address": {"type": "string"},
                                            "port": {"type": "integer", "format": "int32"},
                                            "success": {"type": "boolean"},
                                            "latency_ms": {"type": "integer", "format": "int64", "nullable": true},
                                            "message": {"type": "string"},
                                            "error": {"type": "string", "nullable": true}
                                        }
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "Invalid request payload",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "address": {"type": "string"},
                                            "port": {"type": "integer", "format": "int32"},
                                            "success": {"type": "boolean"},
                                            "latency_ms": {"type": "integer", "format": "int64", "nullable": true},
                                            "message": {"type": "string"},
                                            "error": {"type": "string", "nullable": true}
                                        }
                                    }
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
            },
            "/api/acl/groups": {
                "get": {
                    "summary": "List all ACL groups",
                    "description": "Get a list of all configured ACL groups with rule counts",
                    "tags": ["ACL-Groups"],
                    "operationId": "listGroups",
                    "responses": {
                        "200": {
                            "description": "List of ACL groups",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "groups": {
                                                "type": "array",
                                                "items": {
                                                    "type": "object",
                                                    "properties": {
                                                        "name": {"type": "string", "example": "developers"},
                                                        "rule_count": {"type": "integer", "example": 5}
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    "example": {
                                        "groups": [
                                            {"name": "developers", "rule_count": 3},
                                            {"name": "admins", "rule_count": 5}
                                        ]
                                    }
                                }
                            }
                        }
                    }
                },
                "post": {
                    "summary": "Create new ACL group",
                    "description": "Create a new empty ACL group",
                    "tags": ["ACL-Groups"],
                    "operationId": "createGroup",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "name": {"type": "string", "example": "admins"}
                                    },
                                    "required": ["name"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Group created successfully",
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
                            "description": "Group already exists or ACL not enabled"
                        }
                    }
                }
            },
            "/api/acl/groups/{groupname}": {
                "get": {
                    "summary": "Get ACL group details",
                    "description": "Get detailed information about an ACL group including all rules",
                    "tags": ["ACL-Groups"],
                    "operationId": "getGroupDetail",
                    "parameters": [
                        {
                            "name": "groupname",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Group name",
                            "example": "developers"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Group details with rules",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "name": {"type": "string"},
                                            "rules": {
                                                "type": "array",
                                                "items": {"$ref": "#/components/schemas/AclRule"}
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        "404": {
                            "description": "Group not found"
                        }
                    }
                },
                "delete": {
                    "summary": "Delete ACL group",
                    "description": "Delete an entire ACL group and all its rules",
                    "tags": ["ACL-Groups"],
                    "operationId": "deleteGroup",
                    "parameters": [
                        {
                            "name": "groupname",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Group name"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Group deleted successfully"
                        },
                        "404": {
                            "description": "Group not found"
                        }
                    }
                }
            },
            "/api/acl/groups/{groupname}/rules": {
                "post": {
                    "summary": "Add rule to group",
                    "description": "Add a new ACL rule to a group. Rules are identified by destination + port combination.",
                    "tags": ["ACL-Groups"],
                    "operationId": "addGroupRule",
                    "parameters": [
                        {
                            "name": "groupname",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Group name"
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/AddRuleRequest"},
                                "example": {
                                    "action": "allow",
                                    "description": "SSH access to production",
                                    "destinations": ["*.prod.company.com"],
                                    "ports": ["22"],
                                    "protocols": ["tcp"],
                                    "priority": 200
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Rule added successfully",
                            "content": {
                                "application/json": {
                                    "schema": {"$ref": "#/components/schemas/RuleOperationResponse"}
                                }
                            }
                        },
                        "400": {
                            "description": "Invalid rule or duplicate rule"
                        }
                    }
                },
                "put": {
                    "summary": "Update group rule",
                    "description": "Update an existing rule by identifying it with destination + port",
                    "tags": ["ACL-Groups"],
                    "operationId": "updateGroupRule",
                    "parameters": [
                        {
                            "name": "groupname",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Group name"
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "match": {"$ref": "#/components/schemas/RuleIdentifier"},
                                        "update": {"$ref": "#/components/schemas/AddRuleRequest"}
                                    },
                                    "required": ["match", "update"]
                                },
                                "example": {
                                    "match": {
                                        "destinations": ["*.prod.company.com"],
                                        "ports": ["22"]
                                    },
                                    "update": {
                                        "action": "block",
                                        "description": "SSH blocked (new security policy)",
                                        "destinations": ["*.prod.company.com"],
                                        "ports": ["22"],
                                        "protocols": ["tcp"],
                                        "priority": 1000
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Rule updated successfully",
                            "content": {
                                "application/json": {
                                    "schema": {"$ref": "#/components/schemas/RuleOperationResponse"}
                                }
                            }
                        },
                        "404": {
                            "description": "Rule not found"
                        }
                    }
                },
                "delete": {
                    "summary": "Delete group rule",
                    "description": "Delete a rule by identifying it with destination + port",
                    "tags": ["ACL-Groups"],
                    "operationId": "deleteGroupRule",
                    "parameters": [
                        {
                            "name": "groupname",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Group name"
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/RuleIdentifier"},
                                "example": {
                                    "destinations": ["*.prod.company.com"],
                                    "ports": ["22"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Rule deleted successfully"
                        },
                        "404": {
                            "description": "Rule not found"
                        }
                    }
                }
            },
            "/api/acl/users": {
                "get": {
                    "summary": "List all ACL users",
                    "description": "Get a list of all users with ACL rules configured",
                    "tags": ["ACL-Users"],
                    "operationId": "listUsers",
                    "responses": {
                        "200": {
                            "description": "List of ACL users",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "users": {
                                                "type": "array",
                                                "items": {
                                                    "type": "object",
                                                    "properties": {
                                                        "username": {"type": "string"},
                                                        "groups": {"type": "array", "items": {"type": "string"}},
                                                        "rule_count": {"type": "integer"}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/acl/users/{username}": {
                "get": {
                    "summary": "Get user ACL details",
                    "description": "Get detailed ACL information for a specific user",
                    "tags": ["ACL-Users"],
                    "operationId": "getUserDetail",
                    "parameters": [
                        {
                            "name": "username",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Username"
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "User ACL details"
                        },
                        "404": {
                            "description": "User not found"
                        }
                    }
                }
            },
            "/api/acl/users/{username}/rules": {
                "post": {
                    "summary": "Add rule to user",
                    "description": "Add a per-user ACL rule override (higher priority than group rules)",
                    "tags": ["ACL-Users"],
                    "operationId": "addUserRule",
                    "parameters": [
                        {
                            "name": "username",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Username"
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/AddRuleRequest"},
                                "example": {
                                    "action": "block",
                                    "description": "Alice blocked from admin panel",
                                    "destinations": ["admin.company.com"],
                                    "ports": ["*"],
                                    "protocols": ["tcp"],
                                    "priority": 2000
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Rule added successfully"
                        }
                    }
                },
                "put": {
                    "summary": "Update user rule",
                    "description": "Update an existing per-user rule",
                    "tags": ["ACL-Users"],
                    "operationId": "updateUserRule",
                    "parameters": [
                        {
                            "name": "username",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Username"
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "match": {"$ref": "#/components/schemas/RuleIdentifier"},
                                        "update": {"$ref": "#/components/schemas/AddRuleRequest"}
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Rule updated successfully"
                        },
                        "404": {
                            "description": "Rule not found"
                        }
                    }
                },
                "delete": {
                    "summary": "Delete user rule",
                    "description": "Delete a per-user ACL rule",
                    "tags": ["ACL-Users"],
                    "operationId": "deleteUserRule",
                    "parameters": [
                        {
                            "name": "username",
                            "in": "path",
                            "required": true,
                            "schema": {"type": "string"},
                            "description": "Username"
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/RuleIdentifier"}
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Rule deleted successfully"
                        },
                        "404": {
                            "description": "Rule not found"
                        }
                    }
                }
            },
            "/api/acl/global": {
                "get": {
                    "summary": "Get global ACL settings",
                    "description": "Get global ACL configuration (default policy)",
                    "tags": ["ACL-Global"],
                    "operationId": "getGlobalSettings",
                    "responses": {
                        "200": {
                            "description": "Global ACL settings",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "default_policy": {"type": "string", "enum": ["allow", "block"], "example": "block"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "put": {
                    "summary": "Update global ACL settings",
                    "description": "Update global ACL configuration (default policy)",
                    "tags": ["ACL-Global"],
                    "operationId": "updateGlobalSettings",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "default_policy": {"type": "string", "enum": ["allow", "block"]}
                                    },
                                    "required": ["default_policy"]
                                },
                                "example": {
                                    "default_policy": "allow"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Global settings updated successfully",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "success": {"type": "boolean"},
                                            "message": {"type": "string"},
                                            "old_policy": {"type": "string"},
                                            "new_policy": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/api/acl/search": {
                "post": {
                    "summary": "Search ACL rules",
                    "description": "Search for ACL rules across all groups and users using various criteria",
                    "tags": ["ACL-Global"],
                    "operationId": "searchRules",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "destination": {"type": "string", "example": "prod.company.com"},
                                        "port": {"type": "integer", "example": 22},
                                        "action": {"type": "string", "enum": ["allow", "block"]}
                                    }
                                },
                                "example": {
                                    "destination": "prod.company.com"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Search results",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "matches": {
                                                "type": "array",
                                                "items": {
                                                    "type": "object",
                                                    "properties": {
                                                        "rule_type": {"type": "string", "enum": ["group", "user"]},
                                                        "owner": {"type": "string"},
                                                        "rule": {"$ref": "#/components/schemas/AclRule"}
                                                    }
                                                }
                                            },
                                            "count": {"type": "integer"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "AclRule": {
                    "type": "object",
                    "properties": {
                        "action": {"type": "string", "enum": ["allow", "block"]},
                        "description": {"type": "string"},
                        "destinations": {"type": "array", "items": {"type": "string"}, "example": ["*.example.com", "10.0.0.0/8"]},
                        "ports": {"type": "array", "items": {"type": "string"}, "example": ["22", "80", "443", "8000-9000"]},
                        "protocols": {"type": "array", "items": {"type": "string", "enum": ["tcp", "udp", "both"]}},
                        "priority": {"type": "integer", "example": 100}
                    }
                },
                "AddRuleRequest": {
                    "type": "object",
                    "properties": {
                        "action": {"type": "string", "enum": ["allow", "block"]},
                        "description": {"type": "string"},
                        "destinations": {"type": "array", "items": {"type": "string"}},
                        "ports": {"type": "array", "items": {"type": "string"}},
                        "protocols": {"type": "array", "items": {"type": "string", "enum": ["tcp", "udp", "both"]}},
                        "priority": {"type": "integer"}
                    },
                    "required": ["action", "description", "destinations", "ports", "protocols", "priority"]
                },
                "RuleIdentifier": {
                    "type": "object",
                    "description": "Identifies a rule by destination + optional port (NOT by index!)",
                    "properties": {
                        "destinations": {"type": "array", "items": {"type": "string"}},
                        "ports": {"type": "array", "items": {"type": "string"}}
                    },
                    "required": ["destinations"]
                },
                "RuleOperationResponse": {
                    "type": "object",
                    "properties": {
                        "success": {"type": "boolean"},
                        "message": {"type": "string"},
                        "rule": {"$ref": "#/components/schemas/AclRule"},
                        "old_rule": {"$ref": "#/components/schemas/AclRule"}
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
    connection_pool: Arc<ConnectionPool>,
    metrics_history: Option<Arc<crate::session::MetricsHistory>>,
) -> Result<JoinHandle<()>> {
    if !config.enable_api {
        info!("API server disabled");
        return Err(RustSocksError::Config("API server disabled".to_string()));
    }

    let base_path = if config.base_path.trim().is_empty() {
        "/".to_string()
    } else {
        // Remove trailing slash from base_path (required for nest())
        config.base_path.trim_end_matches('/').to_string()
    };
    let base_prefix = if base_path == "/" {
        ""
    } else {
        base_path.as_str()
    };

    info!(
        "Mounting API router at base path '{}'",
        if base_prefix.is_empty() {
            "/"
        } else {
            base_prefix
        }
    );

    #[cfg(feature = "database")]
    let session_store = session_manager.session_store();

    let state = ApiState {
        session_manager,
        acl_engine,
        acl_config_path,
        start_time: std::time::Instant::now(),
        #[cfg(feature = "database")]
        session_store,
        metrics_history,
        connection_pool,
    };

    // Build router with all endpoints
    let mut app = Router::new();

    // Conditionally add Swagger UI
    if config.swagger_enabled {
        let base_for_swagger = base_prefix.to_string();
        app = app
            .route(
                "/swagger-ui/",
                get(move || swagger_ui(base_for_swagger.clone())),
            )
            .route(
                "/swagger-ui",
                get({
                    let base_for_swagger = base_prefix.to_string();
                    move || swagger_ui(base_for_swagger.clone())
                }),
            )
            .route(
                "/openapi.json",
                get({
                    let base_for_openapi = base_prefix.to_string();
                    move || openapi_spec(base_for_openapi.clone())
                }),
            );
        let swagger_mount = if base_prefix.is_empty() {
            "/swagger-ui/".to_string()
        } else {
            format!("{}/swagger-ui/", base_prefix)
        };
        info!("Swagger UI enabled at {swagger_mount}");
    }

    // Add API routes
    app = app
        // Health and metrics
        .route("/health", get(health_check))
        .route("/metrics", get(get_metrics))
        .route("/api/pool/stats", get(get_pool_stats))
        .route("/api/system/resources", get(get_system_resources))
        // Session endpoints
        .route("/api/sessions/active", get(get_active_sessions))
        .route("/api/sessions/history", get(get_session_history))
        .route("/api/sessions/stats", get(get_session_stats))
        .route("/api/sessions/:id", get(get_session_detail))
        .route("/api/sessions/:id/terminate", post(terminate_session))
        .route("/api/users/:user/sessions", get(get_user_sessions))
        .route("/api/metrics/history", get(get_metrics_history))
        // Diagnostics endpoints
        .route("/api/diagnostics/connectivity", post(test_tcp_connectivity))
        // Management endpoints
        .route("/api/admin/reload-acl", post(reload_acl))
        .route("/api/acl/rules", get(get_acl_rules))
        .route("/api/acl/test", post(test_acl_decision))
        // ACL Management endpoints - Groups
        .route("/api/acl/groups", get(list_groups))
        .route("/api/acl/groups", post(create_group))
        .route("/api/acl/groups/:groupname", get(get_group_detail))
        .route(
            "/api/acl/groups/:groupname",
            axum::routing::delete(delete_group),
        )
        .route("/api/acl/groups/:groupname/rules", post(add_group_rule))
        .route(
            "/api/acl/groups/:groupname/rules",
            axum::routing::put(update_group_rule),
        )
        .route(
            "/api/acl/groups/:groupname/rules",
            axum::routing::delete(delete_group_rule),
        )
        // ACL Management endpoints - Users
        .route("/api/acl/users", get(list_users))
        .route("/api/acl/users/:username", get(get_user_detail))
        .route("/api/acl/users/:username/rules", post(add_user_rule))
        .route(
            "/api/acl/users/:username/rules",
            axum::routing::put(update_user_rule),
        )
        .route(
            "/api/acl/users/:username/rules",
            axum::routing::delete(delete_user_rule),
        )
        // ACL Management endpoints - Global & Search
        .route("/api/acl/global", get(get_global_settings))
        .route(
            "/api/acl/global",
            axum::routing::put(update_global_settings),
        )
        .route("/api/acl/search", post(search_rules));

    // Conditionally serve dashboard static files
    if config.dashboard_enabled {
        let dashboard_path = "dashboard/dist";
        if Path::new(dashboard_path).exists() {
            info!(
                "Dashboard enabled - serving static files from {}",
                dashboard_path
            );

            let index_path = Path::new(dashboard_path).join("index.html");
            let base_for_assets = base_prefix.to_string();

            // Create ServeDir for assets directory only
            let assets_path = format!("{}/assets", dashboard_path);
            let assets_service = ServeDir::new(&assets_path);

            // Handler for serving rewritten index.html (for SPA routing)
            let serve_spa = {
                let index_path = index_path.clone();
                let base_prefix = base_for_assets.clone();
                move || {
                    let index_path = index_path.clone();
                    let base_prefix = base_prefix.clone();
                    async move {
                        match tokio::fs::read_to_string(&index_path).await {
                            Ok(raw) => {
                                let rewritten = rewrite_dashboard_index(&raw, &base_prefix);
                                Ok::<_, StatusCode>(Html(rewritten))
                            }
                            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
                        }
                    }
                }
            };

            // Mount assets directory at /assets
            app = app.nest_service("/assets", assets_service);

            // Serve index.html for root explicitly AND as fallback for SPA routing
            // This must come AFTER all API routes are defined
            app = app.route("/", get(serve_spa.clone()));
            app = app.fallback(serve_spa);
        } else {
            warn!(
                "Dashboard enabled but {} directory not found. Run 'cd dashboard && npm run build'",
                dashboard_path
            );
        }
    }

    let app = if base_path == "/" {
        app
    } else {
        Router::new().nest(&base_path, app)
    };

    // Layer with state and body limit (no path normalization - nginx handles it)
    let app = app
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB max body
        .with_state(state);

    // Bind and listen
    let addr: SocketAddr = format!("{}:{}", config.bind_address, config.bind_port)
        .parse()
        .map_err(|e| RustSocksError::Config(format!("Invalid bind address: {}", e)))?;

    let listener = TcpListener::bind(&addr).await?;

    // Log base URL with base_path if present
    let base_url = if base_prefix.is_empty() {
        format!("http://{}", addr)
    } else {
        format!("http://{}{}", addr, base_prefix)
    };

    info!("API server listening on {}", base_url);

    if config.swagger_enabled {
        info!("Swagger UI available at {}/swagger-ui/", base_url);
    }

    if config.dashboard_enabled {
        info!("Dashboard available at {}", base_url);
    }

    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, app);
        if let Err(err) = server.await {
            error!("API server error: {}", err);
        }
    });

    Ok(handle)
}

fn rewrite_dashboard_index(original: &str, base_prefix: &str) -> String {
    let mut rewritten = if base_prefix.is_empty() {
        original.to_string()
    } else {
        original
            // Rewrite absolute paths
            .replace(
                "href=\"/assets/",
                &format!("href=\"{}/assets/", base_prefix),
            )
            .replace("src=\"/assets/", &format!("src=\"{}/assets/", base_prefix))
            // Rewrite relative paths (Vite builds with base: './')
            .replace(
                "href=\"./assets/",
                &format!("href=\"{}/assets/", base_prefix),
            )
            .replace("src=\"./assets/", &format!("src=\"{}/assets/", base_prefix))
            // Rewrite favicon paths
            .replace(
                "href=\"/vite.svg\"",
                &format!("href=\"{}/vite.svg\"", base_prefix),
            )
            .replace(
                "src=\"/vite.svg\"",
                &format!("src=\"{}/vite.svg\"", base_prefix),
            )
            .replace(
                "href=\"./vite.svg\"",
                &format!("href=\"{}/vite.svg\"", base_prefix),
            )
            .replace(
                "src=\"./vite.svg\"",
                &format!("src=\"{}/vite.svg\"", base_prefix),
            )
    };

    inject_base_path_script(&mut rewritten, base_prefix);
    rewritten
}

fn inject_base_path_script(html: &mut String, base_prefix: &str) {
    let marker = "</head>";
    if let Some(idx) = html.find(marker) {
        let script = if base_prefix.is_empty() {
            "<script>window.__RUSTSOCKS_BASE_PATH__ = '';</script>".to_string()
        } else {
            format!(
                "<script>window.__RUSTSOCKS_BASE_PATH__ = '{}';</script>",
                base_prefix
            )
        };
        html.insert_str(idx, &script);
    }
}
