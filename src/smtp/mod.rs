pub mod client;
pub mod encryption;
pub mod notifications;
#[cfg(feature = "database")]
pub mod repository;
pub mod types;

pub use client::SmtpClient;
pub use notifications::{NotificationDecision, NotificationKind, NotificationSkipReason};
#[cfg(feature = "database")]
pub use repository::SmtpRepository;
pub use types::{format_recipients, parse_recipients, SmtpConfig, SmtpMode};
