use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RustSocksError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Unsupported command: {0}")]
    UnsupportedCommand(u8),

    #[error("Unsupported address type: {0}")]
    UnsupportedAddressType(u8),

    #[error("Invalid request")]
    InvalidRequest,
}

pub type Result<T> = std::result::Result<T, RustSocksError>;

/// API-specific error type with HTTP semantics
#[derive(Debug, Error)]
pub enum ApiError {
    /// Resource not found (404)
    #[error("Not found: {0}")]
    NotFound(String),

    /// Bad request / validation error (400)
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Resource conflict (409)
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Internal server error (500)
    #[error("Internal error: {0}")]
    Internal(String),

    /// Service unavailable / feature not enabled (503)
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),
}

/// JSON error response body
#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub message: String,
}

impl ApiError {
    /// Create a NotFound error
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Create a BadRequest error
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    /// Create a Conflict error
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    /// Create an Internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Create a ServiceUnavailable error
    pub fn service_unavailable(msg: impl Into<String>) -> Self {
        Self::ServiceUnavailable(msg.into())
    }

    /// Get the HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// Get the error type name for the response
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::Conflict(_) => "conflict",
            Self::Internal(_) => "internal_error",
            Self::ServiceUnavailable(_) => "service_unavailable",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ApiErrorResponse {
            error: self.error_type().to_string(),
            message: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}

impl From<String> for ApiError {
    fn from(msg: String) -> Self {
        Self::Internal(msg)
    }
}

impl From<&str> for ApiError {
    fn from(msg: &str) -> Self {
        Self::Internal(msg.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(err.to_string())
    }
}
