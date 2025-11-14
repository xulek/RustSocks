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
