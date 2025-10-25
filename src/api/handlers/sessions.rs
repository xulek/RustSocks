use crate::api::types::{
    DestinationStat, PagedResponse, SessionQueryParams, SessionResponse, SessionStatsResponse,
    UserStat,
};
use crate::session::SessionManager;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use std::sync::Arc;

/// API state containing shared resources
#[derive(Clone)]
pub struct ApiState {
    pub session_manager: Arc<SessionManager>,
    pub acl_engine: Option<Arc<crate::acl::AclEngine>>,
    pub acl_config_path: Option<String>,
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
    let mut sessions = state.session_manager.get_closed_sessions();

    // Apply filters
    if let Some(ref user) = params.user {
        sessions.retain(|s| s.user == *user);
    }

    if let Some(ref dest_ip) = params.dest_ip {
        sessions.retain(|s| s.dest_ip == *dest_ip);
    }

    if let Some(ref status) = params.status {
        sessions.retain(|s| s.status.as_str().to_lowercase() == status.to_lowercase());
    }

    // Apply time filter (hours)
    if let Some(hours) = params.hours {
        let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);
        sessions.retain(|s| {
            if let Some(end) = s.end_time {
                end > cutoff
            } else {
                false
            }
        });
    }

    // Sort by end_time descending (newest first)
    sessions.sort_by(|a, b| match (a.end_time, b.end_time) {
        (Some(at), Some(bt)) => bt.cmp(&at),
        _ => std::cmp::Ordering::Equal,
    });

    // Pagination
    let total = sessions.len() as u64;
    let page_size = params.page_size.max(1).min(1000) as usize;
    let page = params.page.max(1);
    let offset = ((page - 1) as usize) * page_size;
    let total_pages = ((total as f32) / (page_size as f32)).ceil() as u32;

    let data: Vec<SessionResponse> = sessions
        .iter()
        .skip(offset)
        .take(page_size)
        .map(|s| session_to_response(s.clone()))
        .collect();

    let response = PagedResponse {
        data,
        total,
        page,
        page_size: page_size as u32,
        total_pages,
    };

    (StatusCode::OK, Json(response))
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
        let entry = user_stats.entry(session.user.clone()).or_insert((0, 0, 0));
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
    let all_sessions = state.session_manager.get_all_sessions().await;
    let user_sessions: Vec<SessionResponse> = all_sessions
        .iter()
        .filter(|s| s.user == user)
        .map(|s| session_to_response(s.clone()))
        .collect();

    (StatusCode::OK, Json(user_sessions))
}

/// Helper function to convert internal Session to API SessionResponse
fn session_to_response(session: crate::session::Session) -> SessionResponse {
    SessionResponse {
        id: session.session_id.to_string(),
        user: session.user,
        source_ip: session.source_ip.to_string(),
        source_port: session.source_port,
        dest_ip: session.dest_ip,
        dest_port: session.dest_port,
        protocol: session.protocol.as_str().to_string(),
        status: session.status.as_str().to_string(),
        acl_decision: session.acl_decision,
        acl_rule: session.acl_rule_matched,
        bytes_sent: session.bytes_sent,
        bytes_received: session.bytes_received,
        start_time: session.start_time.to_rfc3339(),
        end_time: session.end_time.map(|t| t.to_rfc3339()),
        duration_seconds: session.duration_secs,
    }
}
