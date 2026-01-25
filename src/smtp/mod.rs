pub mod client;
pub mod encryption;
#[cfg(feature = "database")]
pub mod repository;
pub mod types;

pub use client::SmtpClient;
#[cfg(feature = "database")]
pub use repository::SmtpRepository;
pub use types::{SmtpConfig, SmtpMode};
