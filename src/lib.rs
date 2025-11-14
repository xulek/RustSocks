// RustSocks - High-performance SOCKS5 proxy server

#![recursion_limit = "256"]

pub mod acl;
pub mod api;
pub mod auth;
pub mod config;
pub mod protocol;
pub mod qos;
pub mod server;
pub mod session;
pub mod telemetry;
pub mod utils;

// Re-export commonly used types
pub use utils::error::{Result, RustSocksError};
