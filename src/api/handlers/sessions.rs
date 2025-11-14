use crate::api::types::{
    DestinationStat, PagedResponse, SessionQueryParams, SessionResponse, SessionStatsResponse,
    UserStat,
};
use crate::config::Config;
#[cfg(feature = "database")]
use crate::session::SessionFilter;
use crate::session::{Session, SessionManager, SessionStatus};
use crate::telemetry::TelemetryHistory;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration as ChronoDuration, Utc};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

#[cfg(feature = "database")]
use tracing::{error, warn};

/// API state containing shared resources
#[derive(Clone)]
pub struct ApiState {
    pub session_manager: Arc<SessionManager>,
    pub acl_engine: Option<Arc<crate::acl::AclEngine>>,
    pub acl_config_path: Option<String>,
    pub connection_pool: Arc<crate::server::pool::ConnectionPool>,
    pub start_time: std::time::Instant,
    #[cfg(feature = "database")]
    pub session_store: Option<Arc<crate::session::SessionStore>>,
    pub metrics_history: Option<Arc<crate::session::MetricsHistory>>,
    pub telemetry_history: Option<Arc<TelemetryHistory>>,
    pub config_path: Option<PathBuf>,
    pub config_snapshot: Arc<Config>,
    pub original_args: Arc<Vec<std::ffi::OsString>>,
}

/// GET /api/sessions/active - Get active sessions
pub async fn get_active_sessions(
    State(state): State<ApiState>,
) -> (StatusCode, Json<Vec<SessionResponse>>) {
    let sessions = state.session_manager.get_active_sessions().await;
    let responses = sessions
        .iter()
        .map(|s| session_to_response(s.clone()))
        .collect();
    (StatusCode::OK, Json(responses))
}

/// GET /api/sessions/history - Get session history with filtering
pub async fn get_session_history(
    State(state): State<ApiState>,
    Query(params): Query<SessionQueryParams>,
) -> (StatusCode, Json<PagedResponse<SessionResponse>>) {
    let page_size = params.page_size.clamp(1, 1000);
    let page_size_usize = page_size as usize;
    let page = params.page.max(1);
    let offset = ((page - 1) as usize) * page_size_usize;

    let mut status_filter: Option<SessionStatus> = None;
    let mut invalid_status = false;
    if let Some(ref status_str) = params.status {
        match SessionStatus::from_str(status_str) {
            Ok(status) => status_filter = Some(status),
            Err(_) => invalid_status = true,
        }
    }

    if invalid_status {
        let response = PagedResponse {
            data: Vec::new(),
            total: 0,
            page,
            page_size,
            total_pages: 0,
        };
        return (StatusCode::OK, Json(response));
    }

    let cutoff = params
        .hours
        .map(|hours| Utc::now() - ChronoDuration::hours(hours as i64));

    let user_filter = params.user.clone();
    let dest_filter = params.dest_ip.clone();
    let sort_by = params.sort_by.clone();
    let sort_dir = params.sort_dir.clone();

    #[cfg(feature = "database")]
    if let Some(store) = state.session_store.as_ref() {
        match fetch_history_from_store(
            store,
            &state.session_manager,
            &user_filter,
            &dest_filter,
            &status_filter,
            cutoff.as_ref(),
            offset,
            page_size_usize,
            page,
            page_size,
            sort_by.clone(),
            sort_dir.clone(),
        )
        .await
        {
            Ok(response) => return (StatusCode::OK, Json(response)),
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to load session history from persistent store, falling back to in-memory data"
                );
            }
        }
    }

    let response = build_memory_history_response(
        &state.session_manager,
        &user_filter,
        &dest_filter,
        &status_filter,
        cutoff.as_ref(),
        page,
        page_size_usize,
        offset,
        page_size,
        sort_by,
        sort_dir,
    )
    .await;

    (StatusCode::OK, Json(response))
}

#[cfg(feature = "database")]
// Helper needs to accept separate filter and pagination arguments without additional structs.
#[allow(clippy::too_many_arguments)]
async fn fetch_history_from_store(
    store: &crate::session::SessionStore,
    manager: &SessionManager,
    user_filter: &Option<String>,
    dest_filter: &Option<String>,
    status_filter: &Option<SessionStatus>,
    cutoff: Option<&chrono::DateTime<Utc>>,
    offset: usize,
    page_size: usize,
    page: u32,
    page_size_u32: u32,
    sort_by: Option<String>,
    sort_dir: Option<String>,
) -> Result<PagedResponse<SessionResponse>, sqlx::Error> {
    let mut in_memory = manager.get_closed_sessions().await;
    in_memory.retain(|session| {
        matches_history_filters(session, user_filter, dest_filter, status_filter, cutoff)
    });

    let extra_ids: Vec<_> = in_memory.iter().map(|s| s.session_id).collect();
    let persisted_ids = store.existing_session_ids(&extra_ids).await?;

    let mut extra_sessions: Vec<Session> = in_memory
        .into_iter()
        .filter(|session| !persisted_ids.contains(&session.session_id))
        .collect();

    // Apply same sorting as database query
    let sort_field = sort_by.as_deref().unwrap_or("start_time");
    let is_asc = sort_dir
        .as_deref()
        .map(|d| d.eq_ignore_ascii_case("asc"))
        .unwrap_or(false);

    extra_sessions.sort_by(|a, b| {
        let ordering = match sort_field {
            "user" => a.user.cmp(&b.user),
            "source_ip" => a.source_ip.to_string().cmp(&b.source_ip.to_string()),
            "dest_ip" => a.dest_ip.cmp(&b.dest_ip),
            "protocol" => a.protocol.as_str().cmp(b.protocol.as_str()),
            "status" => a.status.as_str().cmp(b.status.as_str()),
            "acl_decision" => a.acl_decision.cmp(&b.acl_decision),
            "bytes_sent" => a.bytes_sent.cmp(&b.bytes_sent),
            "bytes_received" => a.bytes_received.cmp(&b.bytes_received),
            "duration_seconds" | "duration_secs" => a.duration_secs.cmp(&b.duration_secs),
            _ => {
                // Default to start_time
                a.start_time.cmp(&b.start_time)
            }
        };
        if is_asc {
            ordering
        } else {
            ordering.reverse()
        }
    });

    let extra_total = extra_sessions.len();
    let extra_slice_start = extra_total.min(offset);
    let extra_slice_end = extra_total.min(offset + page_size);
    let extra_page_len = extra_slice_end.saturating_sub(extra_slice_start);
    let extra_page: Vec<Session> = extra_sessions[extra_slice_start..extra_slice_end].to_vec();

    let db_offset = offset.saturating_sub(extra_total);
    let db_limit = page_size.saturating_sub(extra_page_len);

    let filter = SessionFilter {
        user: user_filter.clone(),
        dest_ip: dest_filter.clone(),
        status: status_filter.clone(),
        limit: Some(db_limit as u64),
        offset: Some(db_offset as u64),
        start_after: cutoff.cloned(),
        sort_by,
        sort_dir,
        ..Default::default()
    };

    let mut count_filter = filter.clone();
    count_filter.limit = None;
    count_filter.offset = None;
    let db_total = store.count_sessions(&count_filter).await?;

    let db_sessions = if db_limit > 0 {
        store.query_sessions(&filter).await?
    } else {
        Vec::new()
    };

    let mut combined = Vec::with_capacity(extra_page.len() + db_sessions.len());
    combined.extend(extra_page);
    combined.extend(db_sessions);

    let total = db_total + extra_total as u64;
    let total_pages = if total == 0 {
        0
    } else {
        ((total as f64) / (page_size as f64)).ceil() as u32
    };

    let data = combined.into_iter().map(session_to_response).collect();

    Ok(PagedResponse {
        data,
        total,
        page,
        page_size: page_size_u32,
        total_pages,
    })
}

#[allow(clippy::too_many_arguments)]
async fn build_memory_history_response(
    manager: &SessionManager,
    user_filter: &Option<String>,
    dest_filter: &Option<String>,
    status_filter: &Option<SessionStatus>,
    cutoff: Option<&chrono::DateTime<Utc>>,
    page: u32,
    page_size_usize: usize,
    offset: usize,
    page_size_u32: u32,
    sort_by: Option<String>,
    sort_dir: Option<String>,
) -> PagedResponse<SessionResponse> {
    let mut sessions = manager.get_closed_sessions().await;
    sessions.retain(|session| {
        matches_history_filters(session, user_filter, dest_filter, status_filter, cutoff)
    });

    // Apply sorting
    let sort_field = sort_by.as_deref().unwrap_or("start_time");
    let is_asc = sort_dir
        .as_deref()
        .map(|d| d.eq_ignore_ascii_case("asc"))
        .unwrap_or(false);

    sessions.sort_by(|a, b| {
        let ordering = match sort_field {
            "user" => a.user.cmp(&b.user),
            "source_ip" => a.source_ip.to_string().cmp(&b.source_ip.to_string()),
            "dest_ip" => a.dest_ip.cmp(&b.dest_ip),
            "protocol" => a.protocol.as_str().cmp(b.protocol.as_str()),
            "status" => a.status.as_str().cmp(b.status.as_str()),
            "acl_decision" => a.acl_decision.cmp(&b.acl_decision),
            "bytes_sent" => a.bytes_sent.cmp(&b.bytes_sent),
            "bytes_received" => a.bytes_received.cmp(&b.bytes_received),
            "duration_seconds" | "duration_secs" => a.duration_secs.cmp(&b.duration_secs),
            _ => {
                // Default to start_time
                a.start_time.cmp(&b.start_time)
            }
        };
        if is_asc {
            ordering
        } else {
            ordering.reverse()
        }
    });

    let total = sessions.len() as u64;
    let total_pages = if total == 0 {
        0
    } else {
        ((total as f64) / (page_size_usize as f64)).ceil() as u32
    };

    let data = sessions
        .into_iter()
        .skip(offset)
        .take(page_size_usize)
        .map(session_to_response)
        .collect();

    PagedResponse {
        data,
        total,
        page,
        page_size: page_size_u32,
        total_pages,
    }
}

fn matches_history_filters(
    session: &Session,
    user_filter: &Option<String>,
    dest_filter: &Option<String>,
    status_filter: &Option<SessionStatus>,
    cutoff: Option<&chrono::DateTime<Utc>>,
) -> bool {
    if let Some(user) = user_filter.as_ref() {
        if session.user.as_ref() != user {
            return false;
        }
    }

    if let Some(dest) = dest_filter.as_ref() {
        if session.dest_ip.as_ref() != dest {
            return false;
        }
    }

    if let Some(status) = status_filter.as_ref() {
        if &session.status != status {
            return false;
        }
    }

    if let Some(cutoff_time) = cutoff {
        match session.end_time {
            Some(end) if end > *cutoff_time => {}
            _ => return false,
        }
    }

    true
}

/// GET /api/sessions/{id} - Get specific session details
pub async fn get_session_detail(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> axum::response::Result<(StatusCode, Json<SessionResponse>)> {
    let sessions = state.session_manager.get_all_sessions().await;

    if let Some(session) = sessions.iter().find(|s| s.session_id.to_string() == id) {
        Ok((StatusCode::OK, Json(session_to_response(session.clone()))))
    } else {
        #[cfg(feature = "database")]
        {
            if let Some(store) = state.session_store.as_ref() {
                match Uuid::parse_str(&id) {
                    Ok(uuid) => match store.get_session(&uuid).await {
                        Ok(Some(session)) => {
                            return Ok((StatusCode::OK, Json(session_to_response(session))));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!(session_id = %id, error = %e, "Failed to load session from store")
                        }
                    },
                    Err(_) => {
                        return Err((StatusCode::BAD_REQUEST, "Invalid session id").into());
                    }
                }
            }
        }

        Err((StatusCode::NOT_FOUND, "Session not found").into())
    }
}

/// GET /api/sessions/stats - Get aggregated session statistics
pub async fn get_session_stats(
    State(state): State<ApiState>,
) -> (StatusCode, Json<SessionStatsResponse>) {
    let all_sessions = state.session_manager.get_all_sessions().await;

    let active_sessions = all_sessions
        .iter()
        .filter(|s| s.status.as_str() == "active")
        .count() as u64;
    let closed_sessions = all_sessions
        .iter()
        .filter(|s| s.status.as_str() == "closed")
        .count() as u64;
    let failed_sessions = all_sessions
        .iter()
        .filter(|s| s.status.as_str() == "failed")
        .count() as u64;

    let total_bytes_sent: u64 = all_sessions.iter().map(|s| s.bytes_sent).sum();
    let total_bytes_received: u64 = all_sessions.iter().map(|s| s.bytes_received).sum();

    // Calculate top users (by session count)
    let mut user_stats: std::collections::HashMap<String, (u64, u64, u64)> =
        std::collections::HashMap::new();
    for session in &all_sessions {
        let entry = user_stats
            .entry(session.user.to_string())
            .or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += session.bytes_sent;
        entry.2 += session.bytes_received;
    }

    let mut top_users: Vec<UserStat> = user_stats
        .into_iter()
        .map(|(user, (count, sent, received))| UserStat {
            user,
            session_count: count,
            bytes_sent: sent,
            bytes_received: received,
        })
        .collect();
    top_users.sort_by(|a, b| b.session_count.cmp(&a.session_count));
    top_users.truncate(10);

    // Calculate top destinations
    let mut dest_stats: std::collections::HashMap<String, (u64, u64, u64)> =
        std::collections::HashMap::new();
    for session in &all_sessions {
        let key = format!("{}:{}", session.dest_ip, session.dest_port);
        let entry = dest_stats.entry(key).or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += session.bytes_sent;
        entry.2 += session.bytes_received;
    }

    let mut top_destinations: Vec<DestinationStat> = dest_stats
        .into_iter()
        .map(|(destination, (count, sent, received))| DestinationStat {
            destination,
            session_count: count,
            bytes_sent: sent,
            bytes_received: received,
        })
        .collect();
    top_destinations.sort_by(|a, b| b.session_count.cmp(&a.session_count));
    top_destinations.truncate(10);

    let response = SessionStatsResponse {
        total_sessions: all_sessions.len() as u64,
        active_sessions,
        closed_sessions,
        failed_sessions,
        total_bytes_sent,
        total_bytes_received,
        top_users,
        top_destinations,
    };

    (StatusCode::OK, Json(response))
}

/// GET /api/users/{user}/sessions - Get sessions for specific user
pub async fn get_user_sessions(
    State(state): State<ApiState>,
    Path(user): Path<String>,
) -> (StatusCode, Json<Vec<SessionResponse>>) {
    #[cfg(feature = "database")]
    let all_sessions: Vec<Session> = {
        let mut sessions = Vec::new();
        if let Some(store) = state.session_store.as_ref() {
            let filter = SessionFilter {
                user: Some(user.clone()),
                status: None,
                start_after: None,
                start_before: None,
                dest_ip: None,
                min_duration_secs: None,
                min_bytes: None,
                limit: Some(1000), // Limit to 1000 most recent sessions
                offset: None,
                sort_by: Some("start_time".to_string()),
                sort_dir: Some("desc".to_string()),
            };

            match store.query_sessions(&filter).await {
                Ok(db_sessions) => {
                    sessions.extend(db_sessions);
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        user = %user,
                        "Failed to fetch user sessions from database"
                    );
                }
            }
        }
        sessions
    };

    #[cfg(not(feature = "database"))]
    let all_sessions: Vec<Session> = Vec::new();

    // Add active sessions from memory (might overlap with DB, we'll deduplicate)
    let memory_sessions = state.session_manager.get_all_sessions().await;
    let user_memory_sessions: Vec<Session> = memory_sessions
        .into_iter()
        .filter(|s| s.user.as_ref() == user)
        .collect();

    // Deduplicate by session_id (prefer in-memory sessions as they're more up-to-date)
    let mut session_map = std::collections::HashMap::new();

    // Add DB sessions first
    for session in all_sessions {
        session_map.insert(session.session_id, session);
    }

    // Overwrite with memory sessions (more current)
    for session in user_memory_sessions {
        session_map.insert(session.session_id, session);
    }

    // Convert to response and sort by start time (newest first)
    let mut user_sessions: Vec<SessionResponse> =
        session_map.into_values().map(session_to_response).collect();

    user_sessions.sort_by(|a, b| b.start_time.cmp(&a.start_time));

    (StatusCode::OK, Json(user_sessions))
}

/// GET /api/metrics/history - Get historical metrics snapshots
pub async fn get_metrics_history(
    State(state): State<ApiState>,
) -> (StatusCode, Json<Vec<crate::session::MetricsSnapshot>>) {
    // Try to load from database first (persistent)
    #[cfg(feature = "database")]
    if let Some(store) = state.session_store.as_ref() {
        // Get last 2 hours of metrics (1440 snapshots @ 5s intervals)
        match store.query_metrics(None, Some(1440)).await {
            Ok(mut snapshots) => {
                // Reverse to get chronological order (query returns DESC)
                snapshots.reverse();
                return (StatusCode::OK, Json(snapshots));
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to load metrics from database, falling back to in-memory"
                );
            }
        }
    }

    // Fallback to in-memory history
    if let Some(history) = state.metrics_history.as_ref() {
        let snapshots = history.get_snapshots().await;
        (StatusCode::OK, Json(snapshots))
    } else {
        (StatusCode::OK, Json(Vec::new()))
    }
}

/// POST /api/sessions/:id/terminate - Terminate an active session
pub async fn terminate_session(
    State(state): State<ApiState>,
    Path(session_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Parse session ID
    let session_uuid = match Uuid::from_str(&session_id) {
        Ok(uuid) => uuid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid session ID format"
                })),
            );
        }
    };

    // Check if session exists and is active
    let session_exists = state
        .session_manager
        .get_active_sessions()
        .await
        .iter()
        .any(|s| s.session_id == session_uuid);

    if !session_exists {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Session not found or not active"
            })),
        );
    }

    // Terminate the session
    state
        .session_manager
        .terminate_session(&session_uuid, "Terminated by admin", SessionStatus::Closed)
        .await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "message": "Session terminated successfully"
        })),
    )
}

/// Helper function to convert internal Session to API SessionResponse
fn session_to_response(session: crate::session::Session) -> SessionResponse {
    SessionResponse {
        id: session.session_id.to_string(),
        user: session.user.to_string(),
        source_ip: session.source_ip.to_string(),
        source_port: session.source_port,
        dest_ip: session.dest_ip.to_string(),
        dest_port: session.dest_port,
        protocol: session.protocol.as_str().to_string(),
        status: session.status.as_str().to_string(),
        acl_decision: session.acl_decision.to_string(),
        acl_rule: session.acl_rule_matched.as_ref().map(|s| s.to_string()),
        bytes_sent: session.bytes_sent,
        bytes_received: session.bytes_received,
        start_time: session.start_time.to_rfc3339(),
        end_time: session.end_time.map(|t| t.to_rfc3339()),
        duration_seconds: session.duration_secs,
    }
}
