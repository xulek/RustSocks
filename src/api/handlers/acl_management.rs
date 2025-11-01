/// ACL Management API Handlers
///
/// These handlers provide REST API endpoints for managing ACL rules dynamically,
/// including adding, updating, and deleting rules for groups and users.
use crate::acl::crud::{self, RuleIdentifier, RuleSearchCriteria};
use crate::acl::persistence;
use crate::acl::types::{AclRule, Action, Protocol};
use crate::api::handlers::sessions::ApiState;
use crate::api::types::*;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::{error, info};

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert API request to AclRule
fn request_to_rule(req: &AddRuleRequest) -> Result<AclRule, String> {
    // Parse action
    let action = match req.action.to_lowercase().as_str() {
        "allow" => Action::Allow,
        "block" => Action::Block,
        _ => return Err("Invalid action. Must be 'allow' or 'block'".to_string()),
    };

    // Parse protocols
    let protocols: Result<Vec<Protocol>, String> = req
        .protocols
        .iter()
        .map(|p| match p.to_lowercase().as_str() {
            "tcp" => Ok(Protocol::Tcp),
            "udp" => Ok(Protocol::Udp),
            "both" | "*" => Ok(Protocol::Both),
            _ => Err(format!("Invalid protocol: {}", p)),
        })
        .collect();
    let protocols = protocols?;

    // Validate destinations
    if req.destinations.is_empty() {
        return Err("Destinations cannot be empty".to_string());
    }

    // Validate ports
    if req.ports.is_empty() {
        return Err("Ports cannot be empty".to_string());
    }

    Ok(AclRule {
        action,
        description: req.description.clone(),
        destinations: req.destinations.clone(),
        ports: req.ports.clone(),
        protocols,
        priority: req.priority,
    })
}

/// Load current ACL config from file
async fn load_current_config(state: &ApiState) -> Result<crate::acl::types::AclConfig, String> {
    let config_path = state
        .acl_config_path
        .as_ref()
        .ok_or_else(|| "ACL config path not set".to_string())?;

    persistence::load_config(config_path).await
}

/// Save config and reload ACL engine
async fn save_and_reload(
    state: &ApiState,
    config: crate::acl::types::AclConfig,
) -> Result<(), String> {
    let config_path = state
        .acl_config_path
        .as_ref()
        .ok_or_else(|| "ACL config path not set".to_string())?;

    // Validate config
    config.validate()?;

    // Save to file (atomic)
    persistence::save_config(&config, config_path).await?;

    // Reload ACL engine
    if let Some(ref acl_engine) = state.acl_engine {
        acl_engine.reload(config).await?;
    }

    Ok(())
}

// ============================================================================
// Group Management Endpoints
// ============================================================================

/// GET /api/acl/groups - List all groups
pub async fn list_groups(State(state): State<ApiState>) -> (StatusCode, Json<GroupListResponse>) {
    let config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load ACL config: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GroupListResponse { groups: vec![] }),
            );
        }
    };

    let groups: Vec<GroupSummary> = config
        .groups
        .iter()
        .map(|g| GroupSummary {
            name: g.name.clone(),
            rule_count: g.rules.len(),
        })
        .collect();

    (StatusCode::OK, Json(GroupListResponse { groups }))
}

/// GET /api/acl/groups/{groupname} - Get group details
pub async fn get_group_detail(
    State(state): State<ApiState>,
    Path(group_name): Path<String>,
) -> (StatusCode, Json<GroupDetailResponse>) {
    let config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load ACL config: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GroupDetailResponse {
                    name: group_name,
                    rules: vec![],
                }),
            );
        }
    };

    let group = config.groups.iter().find(|g| g.name == group_name);

    match group {
        Some(g) => (
            StatusCode::OK,
            Json(GroupDetailResponse {
                name: g.name.clone(),
                rules: g.rules.clone(),
            }),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(GroupDetailResponse {
                name: group_name,
                rules: vec![],
            }),
        ),
    }
}

/// POST /api/acl/groups/{groupname}/rules - Add rule to group
pub async fn add_group_rule(
    State(state): State<ApiState>,
    Path(group_name): Path<String>,
    Json(request): Json<AddRuleRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    // Check if ACL is enabled
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    // Convert request to AclRule
    let rule = match request_to_rule(&request) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Invalid rule: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Load current config
    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Add rule
    if let Err(e) = crud::add_group_rule(&mut config, &group_name, rule.clone()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: e,
                rule: None,
                old_rule: None,
            }),
        );
    }

    // Save and reload
    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(
        group = group_name,
        destinations = ?rule.destinations,
        "Added rule to group via API"
    );

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Rule added to group '{}'", group_name),
            rule: Some(rule),
            old_rule: None,
        }),
    )
}

/// PUT /api/acl/groups/{groupname}/rules - Update group rule
pub async fn update_group_rule(
    State(state): State<ApiState>,
    Path(group_name): Path<String>,
    Json(request): Json<UpdateRuleRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    // Convert request to rule
    let new_rule = match request_to_rule(&request.update) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Invalid rule: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Create identifier
    let identifier = RuleIdentifier {
        destinations: request.match_rule.destinations,
        ports: request.match_rule.ports,
    };

    // Load config
    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Update rule
    let old_rule =
        match crud::update_group_rule(&mut config, &group_name, &identifier, new_rule.clone()) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(RuleOperationResponse {
                        success: false,
                        message: e,
                        rule: None,
                        old_rule: None,
                    }),
                );
            }
        };

    // Save and reload
    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(
        group = group_name,
        old_destinations = ?old_rule.destinations,
        new_destinations = ?new_rule.destinations,
        "Updated rule in group via API"
    );

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Rule updated in group '{}'", group_name),
            rule: Some(new_rule),
            old_rule: Some(old_rule),
        }),
    )
}

/// DELETE /api/acl/groups/{groupname}/rules - Delete group rule
pub async fn delete_group_rule(
    State(state): State<ApiState>,
    Path(group_name): Path<String>,
    Json(request): Json<DeleteRuleRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    // Create identifier
    let identifier = RuleIdentifier {
        destinations: request.destinations,
        ports: request.ports,
    };

    // Load config
    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Delete rule
    let deleted_rule = match crud::delete_group_rule(&mut config, &group_name, &identifier) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(RuleOperationResponse {
                    success: false,
                    message: e,
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Save and reload
    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(
        group = group_name,
        destinations = ?deleted_rule.destinations,
        "Deleted rule from group via API"
    );

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Rule deleted from group '{}'", group_name),
            rule: Some(deleted_rule),
            old_rule: None,
        }),
    )
}

/// POST /api/acl/groups - Create new group
pub async fn create_group(
    State(state): State<ApiState>,
    Json(request): Json<CreateGroupRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    // Check if group already exists
    if config.groups.iter().any(|g| g.name == request.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Group '{}' already exists", request.name),
                rule: None,
                old_rule: None,
            }),
        );
    }

    // Add empty group
    config.groups.push(crate::acl::types::GroupAcl {
        name: request.name.clone(),
        rules: vec![],
    });

    // Save and reload
    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(group = request.name, "Created new group via API");

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Group '{}' created", request.name),
            rule: None,
            old_rule: None,
        }),
    )
}

/// DELETE /api/acl/groups/{groupname} - Delete entire group
pub async fn delete_group(
    State(state): State<ApiState>,
    Path(group_name): Path<String>,
) -> (StatusCode, Json<DeleteGroupResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(DeleteGroupResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                deleted_group: None,
            }),
        );
    }

    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(DeleteGroupResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    deleted_group: None,
                }),
            );
        }
    };

    // Delete group
    let deleted_group = match crud::delete_group(&mut config, &group_name) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(DeleteGroupResponse {
                    success: false,
                    message: e,
                    deleted_group: None,
                }),
            );
        }
    };

    // Save and reload
    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(DeleteGroupResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                deleted_group: None,
            }),
        );
    }

    info!(group = group_name, "Deleted group via API");

    (
        StatusCode::OK,
        Json(DeleteGroupResponse {
            success: true,
            message: format!("Group '{}' deleted", group_name),
            deleted_group: Some(deleted_group),
        }),
    )
}

// ============================================================================
// User Management Endpoints
// ============================================================================

/// GET /api/acl/users - List all users
pub async fn list_users(State(state): State<ApiState>) -> (StatusCode, Json<UserListResponse>) {
    let config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load ACL config: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(UserListResponse { users: vec![] }),
            );
        }
    };

    let users: Vec<UserSummary> = config
        .users
        .iter()
        .map(|u| UserSummary {
            username: u.username.clone(),
            groups: u.groups.clone(),
            rule_count: u.rules.len(),
        })
        .collect();

    (StatusCode::OK, Json(UserListResponse { users }))
}

/// GET /api/acl/users/{username} - Get user details
pub async fn get_user_detail(
    State(state): State<ApiState>,
    Path(username): Path<String>,
) -> (StatusCode, Json<UserDetailResponse>) {
    let config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load ACL config: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(UserDetailResponse {
                    username,
                    groups: vec![],
                    rules: vec![],
                }),
            );
        }
    };

    let user = config.users.iter().find(|u| u.username == username);

    match user {
        Some(u) => (
            StatusCode::OK,
            Json(UserDetailResponse {
                username: u.username.clone(),
                groups: u.groups.clone(),
                rules: u.rules.clone(),
            }),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(UserDetailResponse {
                username,
                groups: vec![],
                rules: vec![],
            }),
        ),
    }
}

/// POST /api/acl/users/{username}/rules - Add rule to user
pub async fn add_user_rule(
    State(state): State<ApiState>,
    Path(username): Path<String>,
    Json(request): Json<AddRuleRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    let rule = match request_to_rule(&request) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Invalid rule: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    if let Err(e) = crud::add_user_rule(&mut config, &username, rule.clone()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: e,
                rule: None,
                old_rule: None,
            }),
        );
    }

    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(
        user = username,
        destinations = ?rule.destinations,
        "Added rule to user via API"
    );

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Rule added to user '{}'", username),
            rule: Some(rule),
            old_rule: None,
        }),
    )
}

/// PUT /api/acl/users/{username}/rules - Update user rule
pub async fn update_user_rule(
    State(state): State<ApiState>,
    Path(username): Path<String>,
    Json(request): Json<UpdateRuleRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    let new_rule = match request_to_rule(&request.update) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Invalid rule: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    let identifier = RuleIdentifier {
        destinations: request.match_rule.destinations,
        ports: request.match_rule.ports,
    };

    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    let old_rule =
        match crud::update_user_rule(&mut config, &username, &identifier, new_rule.clone()) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(RuleOperationResponse {
                        success: false,
                        message: e,
                        rule: None,
                        old_rule: None,
                    }),
                );
            }
        };

    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(
        user = username,
        old_destinations = ?old_rule.destinations,
        new_destinations = ?new_rule.destinations,
        "Updated rule for user via API"
    );

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Rule updated for user '{}'", username),
            rule: Some(new_rule),
            old_rule: Some(old_rule),
        }),
    )
}

/// DELETE /api/acl/users/{username}/rules - Delete user rule
pub async fn delete_user_rule(
    State(state): State<ApiState>,
    Path(username): Path<String>,
    Json(request): Json<DeleteRuleRequest>,
) -> (StatusCode, Json<RuleOperationResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(RuleOperationResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                rule: None,
                old_rule: None,
            }),
        );
    }

    let identifier = RuleIdentifier {
        destinations: request.destinations,
        ports: request.ports,
    };

    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleOperationResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    let deleted_rule = match crud::delete_user_rule(&mut config, &username, &identifier) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(RuleOperationResponse {
                    success: false,
                    message: e,
                    rule: None,
                    old_rule: None,
                }),
            );
        }
    };

    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuleOperationResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                rule: None,
                old_rule: None,
            }),
        );
    }

    info!(
        user = username,
        destinations = ?deleted_rule.destinations,
        "Deleted rule from user via API"
    );

    (
        StatusCode::OK,
        Json(RuleOperationResponse {
            success: true,
            message: format!("Rule deleted from user '{}'", username),
            rule: Some(deleted_rule),
            old_rule: None,
        }),
    )
}

// ============================================================================
// Global Settings & Search
// ============================================================================

/// GET /api/acl/global - Get global settings
pub async fn get_global_settings(
    State(state): State<ApiState>,
) -> (StatusCode, Json<GlobalSettingsResponse>) {
    let config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load ACL config: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(GlobalSettingsResponse {
                    default_policy: "unknown".to_string(),
                }),
            );
        }
    };

    let policy_str = match config.global.default_policy {
        Action::Allow => "allow",
        Action::Block => "block",
    };

    (
        StatusCode::OK,
        Json(GlobalSettingsResponse {
            default_policy: policy_str.to_string(),
        }),
    )
}

/// PUT /api/acl/global - Update global settings
pub async fn update_global_settings(
    State(state): State<ApiState>,
    Json(request): Json<UpdateGlobalSettingsRequest>,
) -> (StatusCode, Json<UpdateGlobalSettingsResponse>) {
    if state.acl_engine.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(UpdateGlobalSettingsResponse {
                success: false,
                message: "ACL is not enabled".to_string(),
                old_policy: "unknown".to_string(),
                new_policy: "unknown".to_string(),
            }),
        );
    }

    let new_policy = match request.default_policy.to_lowercase().as_str() {
        "allow" => Action::Allow,
        "block" => Action::Block,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(UpdateGlobalSettingsResponse {
                    success: false,
                    message: "Invalid policy. Must be 'allow' or 'block'".to_string(),
                    old_policy: "unknown".to_string(),
                    new_policy: request.default_policy,
                }),
            );
        }
    };

    let mut config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(UpdateGlobalSettingsResponse {
                    success: false,
                    message: format!("Failed to load config: {}", e),
                    old_policy: "unknown".to_string(),
                    new_policy: "unknown".to_string(),
                }),
            );
        }
    };

    let old_policy_str = match config.global.default_policy {
        Action::Allow => "allow",
        Action::Block => "block",
    };

    config.global.default_policy = new_policy;

    if let Err(e) = save_and_reload(&state, config).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UpdateGlobalSettingsResponse {
                success: false,
                message: format!("Failed to save config: {}", e),
                old_policy: old_policy_str.to_string(),
                new_policy: request.default_policy,
            }),
        );
    }

    info!(
        old_policy = old_policy_str,
        new_policy = request.default_policy,
        "Updated global ACL policy via API"
    );

    (
        StatusCode::OK,
        Json(UpdateGlobalSettingsResponse {
            success: true,
            message: "Global policy updated".to_string(),
            old_policy: old_policy_str.to_string(),
            new_policy: request.default_policy,
        }),
    )
}

/// POST /api/acl/search - Search for rules
pub async fn search_rules(
    State(state): State<ApiState>,
    Json(request): Json<RuleSearchRequest>,
) -> (StatusCode, Json<RuleSearchResponse>) {
    let config = match load_current_config(&state).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load ACL config: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RuleSearchResponse {
                    matches: vec![],
                    count: 0,
                }),
            );
        }
    };

    let criteria = RuleSearchCriteria {
        destination: request.destination,
        port: request.port,
        action: request.action,
    };

    let results = crud::search_rules(&config, &criteria);

    let matches: Vec<RuleSearchResultItem> = results
        .into_iter()
        .map(|r| RuleSearchResultItem {
            rule_type: r.rule_type,
            owner: r.owner,
            rule: r.rule,
        })
        .collect();

    let count = matches.len();

    (StatusCode::OK, Json(RuleSearchResponse { matches, count }))
}
