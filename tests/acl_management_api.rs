use axum::{
    body::{to_bytes, Body, Bytes},
    http::{Request, StatusCode},
    routing::{delete, get, post, put},
    Router,
};
use rustsocks::acl::types::{AclConfig, AclRule, Action, GlobalAclConfig, GroupAcl, Protocol, UserAcl};
use rustsocks::acl::{load_config, save_config, AclEngine};
use rustsocks::api::handlers::acl_management::{
    add_group_rule, add_user_rule, add_user_to_group, create_group, create_user, delete_group,
    delete_group_rule, delete_user, delete_user_rule, get_global_settings, get_group_detail,
    get_user_detail, list_groups, list_users, remove_user_from_group, search_rules,
    update_global_settings, update_group_rule, update_user_rule,
};
use rustsocks::api::handlers::sessions::ApiState;
use rustsocks::api::types::{
    GroupDetailResponse, GroupListResponse, RuleOperationResponse, RuleSearchResponse,
    UpdateGlobalSettingsResponse, UserDetailResponse, UserListResponse, UserGroupOperationResponse,
};
use rustsocks::config::Config;
use rustsocks::server::pool::{ConnectionPool, PoolConfig};
use rustsocks::session::SessionManager;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower::util::ServiceExt;

fn base_config() -> AclConfig {
    AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Block,
        },
        groups: vec![GroupAcl {
            name: "devs".to_string(),
            rules: vec![],
        }],
        users: vec![UserAcl {
            username: "alice".to_string(),
            groups: vec![],
            rules: vec![],
        }],
    }
}

async fn setup_state_with_config(config: AclConfig) -> (ApiState, TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("acl.toml");
    save_config(&config, &config_path).await.unwrap();

    let acl_engine = Arc::new(AclEngine::new(config).unwrap());
    let connection_pool = Arc::new(ConnectionPool::new(PoolConfig::default()));
    let state = ApiState {
        session_manager: Arc::new(SessionManager::new()),
        acl_engine: Some(acl_engine),
        acl_config_path: Some(config_path.to_string_lossy().to_string()),
        connection_pool,
        start_time: std::time::Instant::now(),
        #[cfg(feature = "database")]
        session_store: None,
        metrics_history: None,
        telemetry_history: None,
        config_path: None,
        config_snapshot: Arc::new(Config::default()),
        original_args: Arc::new(Vec::new()),
    };

    (state, temp_dir, config_path)
}

fn build_router(state: ApiState) -> Router {
    Router::new()
        .route("/api/acl/groups", get(list_groups))
        .route("/api/acl/groups", post(create_group))
        .route("/api/acl/groups/{groupname}", get(get_group_detail))
        .route("/api/acl/groups/{groupname}", delete(delete_group))
        .route("/api/acl/groups/{groupname}/rules", post(add_group_rule))
        .route("/api/acl/groups/{groupname}/rules", put(update_group_rule))
        .route("/api/acl/groups/{groupname}/rules", delete(delete_group_rule))
        .route("/api/acl/users", get(list_users))
        .route("/api/acl/users", post(create_user))
        .route("/api/acl/users/{username}", get(get_user_detail))
        .route("/api/acl/users/{username}", delete(delete_user))
        .route("/api/acl/users/{username}/rules", post(add_user_rule))
        .route("/api/acl/users/{username}/rules", put(update_user_rule))
        .route("/api/acl/users/{username}/rules", delete(delete_user_rule))
        .route("/api/acl/users/{username}/groups", post(add_user_to_group))
        .route(
            "/api/acl/users/{username}/groups/{groupname}",
            delete(remove_user_from_group),
        )
        .route("/api/acl/global", get(get_global_settings))
        .route("/api/acl/global", put(update_global_settings))
        .route("/api/acl/search", post(search_rules))
        .with_state(state)
}

async fn send_json(
    app: &Router,
    method: &str,
    uri: &str,
    payload: serde_json::Value,
) -> (StatusCode, Bytes) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, body)
}

async fn send_empty(app: &Router, uri: &str) -> (StatusCode, Bytes) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, body)
}

#[tokio::test]
async fn test_list_groups_and_group_detail() {
    let (state, _temp_dir, _config_path) = setup_state_with_config(base_config()).await;
    let app = build_router(state);

    let (status, body) = send_empty(&app, "/api/acl/groups").await;
    assert_eq!(status, StatusCode::OK);
    let list: GroupListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(list.groups.len(), 1);
    assert_eq!(list.groups[0].name, "devs");
    assert_eq!(list.groups[0].rule_count, 0);

    let (status, body) = send_empty(&app, "/api/acl/groups/devs").await;
    assert_eq!(status, StatusCode::OK);
    let detail: GroupDetailResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail.name, "devs");
    assert!(detail.rules.is_empty());
}

#[tokio::test]
async fn test_create_group_and_duplicate_fails() {
    let (state, _temp_dir, config_path) = setup_state_with_config(base_config()).await;
    let app = build_router(state);

    let (status, body) = send_json(&app, "POST", "/api/acl/groups", json!({"name": "ops"})).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);
    assert_eq!(response.message, "Group 'ops' created");

    let config = load_config(&config_path).await.unwrap();
    assert!(config.groups.iter().any(|g| g.name == "ops"));

    let (status, body) = send_json(&app, "POST", "/api/acl/groups", json!({"name": "ops"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(!response.success);
    assert_eq!(response.message, "Group 'ops' already exists");
}

#[tokio::test]
async fn test_group_rule_crud() {
    let (state, _temp_dir, config_path) = setup_state_with_config(base_config()).await;
    let app = build_router(state);

    let add_rule = json!({
        "action": "allow",
        "description": "Allow HTTPS",
        "destinations": ["*.example.com"],
        "ports": ["443"],
        "protocols": ["tcp"],
        "priority": 100
    });

    let (status, body) =
        send_json(&app, "POST", "/api/acl/groups/devs/rules", add_rule).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);
    assert_eq!(
        response.rule.as_ref().unwrap().destinations,
        vec!["*.example.com"]
    );

    let update_rule = json!({
        "match": {
            "destinations": ["*.example.com"],
            "ports": ["443"]
        },
        "update": {
            "action": "block",
            "description": "Block HTTPS",
            "destinations": ["*.example.com"],
            "ports": ["443"],
            "protocols": ["tcp"],
            "priority": 200
        }
    });

    let (status, body) =
        send_json(&app, "PUT", "/api/acl/groups/devs/rules", update_rule).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);
    assert_eq!(response.old_rule.as_ref().unwrap().action, Action::Allow);
    assert_eq!(response.rule.as_ref().unwrap().action, Action::Block);

    let delete_rule = json!({
        "destinations": ["*.example.com"],
        "ports": ["443"]
    });

    let (status, body) =
        send_json(&app, "DELETE", "/api/acl/groups/devs/rules", delete_rule).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);

    let config = load_config(&config_path).await.unwrap();
    let group = config.groups.iter().find(|g| g.name == "devs").unwrap();
    assert!(group.rules.is_empty());
}

#[tokio::test]
async fn test_user_list_create_delete() {
    let (state, _temp_dir, config_path) = setup_state_with_config(base_config()).await;
    let app = build_router(state);

    let (status, body) = send_empty(&app, "/api/acl/users").await;
    assert_eq!(status, StatusCode::OK);
    let list: UserListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(list.users.len(), 1);
    assert_eq!(list.users[0].username, "alice");

    let (status, body) = send_empty(&app, "/api/acl/users/alice").await;
    assert_eq!(status, StatusCode::OK);
    let detail: UserDetailResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail.username, "alice");
    assert!(detail.rules.is_empty());

    let (status, body) =
        send_json(&app, "POST", "/api/acl/users", json!({"username": "bob"})).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);

    let (status, body) = send_empty(&app, "/api/acl/users/bob").await;
    assert_eq!(status, StatusCode::OK);
    let detail: UserDetailResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail.username, "bob");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/acl/users/bob")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let config = load_config(&config_path).await.unwrap();
    assert!(!config.users.iter().any(|u| u.username == "bob"));
}

#[tokio::test]
async fn test_user_rule_crud_and_group_assignment() {
    let (state, _temp_dir, config_path) = setup_state_with_config(base_config()).await;
    let app = build_router(state);

    let add_rule = json!({
        "action": "allow",
        "description": "Allow SSH",
        "destinations": ["10.0.0.0/8"],
        "ports": ["22"],
        "protocols": ["tcp"],
        "priority": 50
    });

    let (status, body) =
        send_json(&app, "POST", "/api/acl/users/alice/rules", add_rule).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);

    let update_rule = json!({
        "match": {
            "destinations": ["10.0.0.0/8"],
            "ports": ["22"]
        },
        "update": {
            "action": "block",
            "description": "Block SSH",
            "destinations": ["10.0.0.0/8"],
            "ports": ["22"],
            "protocols": ["tcp"],
            "priority": 60
        }
    });

    let (status, body) =
        send_json(&app, "PUT", "/api/acl/users/alice/rules", update_rule).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);
    assert_eq!(response.rule.as_ref().unwrap().action, Action::Block);

    let delete_rule = json!({
        "destinations": ["10.0.0.0/8"],
        "ports": ["22"]
    });

    let (status, body) =
        send_json(&app, "DELETE", "/api/acl/users/alice/rules", delete_rule).await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);

    let (status, body) = send_json(
        &app,
        "POST",
        "/api/acl/users/alice/groups",
        json!({"group_name": "devs"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response: UserGroupOperationResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/acl/users/alice/groups/devs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let config = load_config(&config_path).await.unwrap();
    let user = config.users.iter().find(|u| u.username == "alice").unwrap();
    assert!(user.rules.is_empty());
    assert!(user.groups.is_empty());
}

#[tokio::test]
async fn test_global_settings_update_and_invalid() {
    let (state, _temp_dir, config_path) = setup_state_with_config(base_config()).await;
    let app = build_router(state);

    let (status, body) = send_empty(&app, "/api/acl/global").await;
    assert_eq!(status, StatusCode::OK);
    let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(response["default_policy"], "block");

    let (status, body) = send_json(
        &app,
        "PUT",
        "/api/acl/global",
        json!({"default_policy": "allow"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response: UpdateGlobalSettingsResponse = serde_json::from_slice(&body).unwrap();
    assert!(response.success);
    assert_eq!(response.old_policy, "block");
    assert_eq!(response.new_policy, "allow");

    let config = load_config(&config_path).await.unwrap();
    assert_eq!(config.global.default_policy, Action::Allow);

    let (status, body) = send_json(
        &app,
        "PUT",
        "/api/acl/global",
        json!({"default_policy": "invalid"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let response: UpdateGlobalSettingsResponse = serde_json::from_slice(&body).unwrap();
    assert!(!response.success);
}

#[tokio::test]
async fn test_search_rules_returns_matches() {
    let config = AclConfig {
        global: GlobalAclConfig {
            default_policy: Action::Block,
        },
        groups: vec![GroupAcl {
            name: "devs".to_string(),
            rules: vec![AclRule {
                action: Action::Allow,
                description: "Allow HTTPS".to_string(),
                destinations: vec!["example.com".to_string()],
                ports: vec!["443".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 100,
            }],
        }],
        users: vec![UserAcl {
            username: "alice".to_string(),
            groups: vec![],
            rules: vec![AclRule {
                action: Action::Block,
                description: "Block SSH".to_string(),
                destinations: vec!["example.com".to_string()],
                ports: vec!["22".to_string()],
                protocols: vec![Protocol::Tcp],
                priority: 200,
            }],
        }],
    };

    let (state, _temp_dir, _config_path) = setup_state_with_config(config).await;
    let app = build_router(state);

    let (status, body) = send_json(
        &app,
        "POST",
        "/api/acl/search",
        json!({"destination": "example.com", "port": 443, "action": "allow"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response: RuleSearchResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.count, 1);
    assert_eq!(response.matches[0].rule_type, "group");
    assert_eq!(response.matches[0].owner, "devs");
}
