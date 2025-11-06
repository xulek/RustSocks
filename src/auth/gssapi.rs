//! GSS-API authentication backend (RFC 1961)
//!
//! Provides Kerberos authentication for SOCKS5 using the GSS-API protocol.

use crate::config::GssApiSettings;
use crate::protocol::{
    parse_gssapi_message, send_gssapi_abort, send_gssapi_message, GssApiMessageType,
    GssApiProtectionLevel,
};
use std::fmt;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{debug, error, info, trace, warn};

#[cfg(unix)]
use libgssapi::{
    context::{SecurityContext, ServerCtx},
    credential::{Cred, CredUsage},
};

/// GSS-API authentication error types
#[derive(Debug)]
pub enum GssApiAuthError {
    Config(String),
    System(String),
    AuthFailed(String),
    #[allow(dead_code)] // Only used on non-Unix platforms
    NotSupported(String),
}

impl fmt::Display for GssApiAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GssApiAuthError::Config(msg) => write!(f, "GSS-API configuration error: {}", msg),
            GssApiAuthError::System(msg) => write!(f, "GSS-API system error: {}", msg),
            GssApiAuthError::AuthFailed(msg) => {
                write!(f, "GSS-API authentication failed: {}", msg)
            }
            GssApiAuthError::NotSupported(msg) => {
                write!(f, "GSS-API not supported: {}", msg)
            }
        }
    }
}

impl std::error::Error for GssApiAuthError {}

/// GSS-API authenticator for SOCKS5 (RFC 1961)
#[derive(Clone)]
pub struct GssApiAuthenticator {
    #[allow(dead_code)] // TODO: Use for service principal specification
    service_name: String,
    protection_level: GssApiProtectionLevel,
    #[allow(dead_code)]
    settings: GssApiSettings,
}

impl GssApiAuthenticator {
    /// Create a new GSS-API authenticator
    pub fn new(settings: &GssApiSettings) -> std::result::Result<Self, GssApiAuthError> {
        #[cfg(not(unix))]
        {
            let _ = settings; // Avoid unused variable warning
            return Err(GssApiAuthError::NotSupported(
                "GSS-API is only supported on Unix systems".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            // Validate service name
            if settings.service_name.is_empty() {
                return Err(GssApiAuthError::Config(
                    "Service name cannot be empty".to_string(),
                ));
            }

            // Validate protection level
            let protection_level = match settings.protection_level.as_str() {
                "integrity" => GssApiProtectionLevel::Integrity,
                "confidentiality" => GssApiProtectionLevel::Confidentiality,
                "selective" => GssApiProtectionLevel::Selective,
                other => {
                    return Err(GssApiAuthError::Config(format!(
                        "Invalid protection level: {} (must be 'integrity', 'confidentiality', or 'selective')",
                        other
                    )));
                }
            };

            debug!(
                service_name = %settings.service_name,
                protection_level = ?protection_level,
                "Initialized GSS-API authenticator"
            );

            Ok(Self {
                service_name: settings.service_name.clone(),
                protection_level,
                settings: settings.clone(),
            })
        }
    }

    /// Perform server-side GSS-API authentication (RFC 1961)
    ///
    /// Returns the authenticated username and groups on success.
    pub async fn authenticate<S>(
        &self,
        stream: &mut S,
    ) -> std::result::Result<(String, Vec<String>), GssApiAuthError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        #[cfg(not(unix))]
        {
            let _ = stream; // Avoid unused variable warning
            return Err(GssApiAuthError::NotSupported(
                "GSS-API is only supported on Unix systems".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            self.authenticate_unix(stream).await
        }
    }

    #[cfg(unix)]
    async fn authenticate_unix<S>(
        &self,
        stream: &mut S,
    ) -> std::result::Result<(String, Vec<String>), GssApiAuthError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use tokio::task;

        debug!("Starting GSS-API authentication");

        // Set keytab path if configured
        if let Some(ref keytab_path) = self.settings.keytab_path {
            debug!(keytab = %keytab_path, "Setting keytab path for GSS-API");
            std::env::set_var("KRB5_KTNAME", keytab_path);
        }

        // Context establishment loop
        let mut server_ctx: Option<ServerCtx> = None;
        #[allow(unused_assignments)]
        let mut username: Option<String> = None;

        loop {
            // Receive authentication token from client
            let client_msg = parse_gssapi_message(stream).await.map_err(|e| {
                GssApiAuthError::System(format!("Failed to parse GSS-API message: {}", e))
            })?;

            // Check for abort
            if client_msg.message_type == GssApiMessageType::Abort {
                warn!("Client sent GSS-API abort message");
                return Err(GssApiAuthError::AuthFailed(
                    "Client aborted authentication".to_string(),
                ));
            }

            // Verify message type
            if client_msg.message_type != GssApiMessageType::Authentication {
                error!(
                    "Unexpected GSS-API message type: {:?}",
                    client_msg.message_type
                );
                send_gssapi_abort(stream)
                    .await
                    .map_err(|e| GssApiAuthError::System(format!("Failed to send abort: {}", e)))?;
                return Err(GssApiAuthError::AuthFailed(format!(
                    "Unexpected message type: {:?}",
                    client_msg.message_type
                )));
            }

            trace!(
                "Received authentication token ({} bytes)",
                client_msg.token.len()
            );

            // Process token in blocking task
            let token = client_msg.token.clone();
            let current_ctx = server_ctx.take();
            let ctx_result = task::spawn_blocking(move || {
                let mut ctx = current_ctx.unwrap_or_else(|| {
                    // Acquire credentials again for new context
                    // This is fast as credentials are cached by the system
                    let cred = Cred::acquire(None, None, CredUsage::Accept, None)
                        .expect("Failed to acquire credentials for new context");
                    ServerCtx::new(cred)
                });
                let output_token = ctx.step(&token)?;
                Ok::<_, libgssapi::error::Error>((ctx, output_token))
            })
            .await
            .map_err(|e| GssApiAuthError::System(format!("Task join error: {}", e)))?;

            match ctx_result {
                Ok((mut ctx, output_token)) => {
                    // Send response token to client
                    let response_token = output_token.as_deref().unwrap_or(&[]);

                    if ctx.is_complete() {
                        debug!("GSS-API context established");

                        // Send final token (may be empty)
                        send_gssapi_message(
                            stream,
                            GssApiMessageType::Authentication,
                            response_token,
                        )
                        .await
                        .map_err(|e| {
                            GssApiAuthError::System(format!("Failed to send response: {}", e))
                        })?;

                        // Get authenticated username
                        let src_name = ctx.source_name().map_err(|e| {
                            GssApiAuthError::System(format!("Failed to get source name: {}", e))
                        })?;
                        username = Some(src_name.to_string());

                        server_ctx = Some(ctx);
                        break; // Context established, proceed to protection negotiation
                    } else {
                        // Continue authentication
                        trace!(
                            "Sending continuation token ({} bytes)",
                            response_token.len()
                        );
                        send_gssapi_message(
                            stream,
                            GssApiMessageType::Authentication,
                            response_token,
                        )
                        .await
                        .map_err(|e| {
                            GssApiAuthError::System(format!("Failed to send response: {}", e))
                        })?;

                        server_ctx = Some(ctx);
                    }
                }
                Err(e) => {
                    error!("GSS-API context step failed: {}", e);
                    send_gssapi_abort(stream).await.map_err(|e2| {
                        GssApiAuthError::System(format!("Failed to send abort: {}", e2))
                    })?;
                    return Err(GssApiAuthError::AuthFailed(format!(
                        "Context establishment failed: {}",
                        e
                    )));
                }
            }
        }

        let username = username.ok_or_else(|| {
            GssApiAuthError::AuthFailed("No username obtained from GSS-API context".to_string())
        })?;

        let mut ctx = server_ctx
            .ok_or_else(|| GssApiAuthError::AuthFailed("No context established".to_string()))?;

        info!(user = %username, "GSS-API authentication successful");

        // Protection level negotiation (RFC 1961 Section 4)
        self.negotiate_protection_level(stream, &mut ctx).await?;

        // Retrieve user groups from system (LDAP via NSS/SSSD)
        let groups = crate::auth::get_user_groups(&username).unwrap_or_else(|e| {
            warn!(
                user = %username,
                error = %e,
                "Failed to retrieve user groups from system, using empty list"
            );
            Vec::new()
        });

        debug!(
            user = %username,
            group_count = groups.len(),
            groups = ?groups,
            "Retrieved user groups from system"
        );

        Ok((username, groups))
    }

    #[cfg(unix)]
    async fn negotiate_protection_level<S>(
        &self,
        stream: &mut S,
        ctx: &mut ServerCtx,
    ) -> std::result::Result<(), GssApiAuthError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        use tokio::task;

        debug!("Starting protection level negotiation");

        // Receive client's protection level proposal
        let client_msg = parse_gssapi_message(stream).await.map_err(|e| {
            GssApiAuthError::System(format!("Failed to parse protection message: {}", e))
        })?;

        if client_msg.message_type != GssApiMessageType::ProtectionLevelNegotiation {
            error!(
                "Expected protection negotiation message, got {:?}",
                client_msg.message_type
            );
            return Err(GssApiAuthError::AuthFailed(
                "Invalid message type during protection negotiation".to_string(),
            ));
        }

        // Move ctx into spawn_blocking and get it back
        let token = client_msg.token.clone();
        let server_protection_level = self.protection_level;
        let mut ctx_moved = std::mem::replace(
            ctx,
            ServerCtx::new(Cred::acquire(None, None, CredUsage::Accept, None).unwrap()),
        );

        let result = task::spawn_blocking(
            move || -> Result<(ServerCtx, u8, Vec<u8>), libgssapi::error::Error> {
                // Unwrap the protection level
                let protection_data = ctx_moved.unwrap(&token)?;
                let protection_byte = if protection_data.is_empty() {
                    0x01 // Default to integrity
                } else {
                    protection_data[0]
                };

                let client_level = GssApiProtectionLevel::from(protection_byte);

                // Server chooses the protection level
                let chosen_level = match (client_level, server_protection_level) {
                    (GssApiProtectionLevel::Integrity, _)
                    | (_, GssApiProtectionLevel::Integrity) => GssApiProtectionLevel::Integrity,
                    (
                        GssApiProtectionLevel::Confidentiality,
                        GssApiProtectionLevel::Confidentiality,
                    ) => GssApiProtectionLevel::Confidentiality,
                    _ => GssApiProtectionLevel::Integrity,
                };

                // Wrap the chosen level
                let level_byte = chosen_level as u8;
                let wrapped_token = ctx_moved.wrap(false, &[level_byte])?;

                Ok((ctx_moved, level_byte, wrapped_token.to_vec()))
            },
        )
        .await
        .map_err(|e| GssApiAuthError::System(format!("Task join error: {}", e)))?
        .map_err(|e| {
            GssApiAuthError::AuthFailed(format!("Failed to negotiate protection level: {}", e))
        })?;

        let (ctx_restored, level_byte, wrapped_token) = result;
        *ctx = ctx_restored;

        let chosen_level = GssApiProtectionLevel::from(level_byte);

        debug!(
            chosen_level = ?chosen_level,
            "Negotiating protection level"
        );

        // Send response
        send_gssapi_message(
            stream,
            GssApiMessageType::ProtectionLevelNegotiation,
            &wrapped_token,
        )
        .await
        .map_err(|e| GssApiAuthError::System(format!("Failed to send protection level: {}", e)))?;

        info!(
            chosen_level = ?chosen_level,
            "Protection level negotiation complete"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gssapi_authenticator_creation() {
        let settings = GssApiSettings {
            service_name: "socks@example.com".to_string(),
            keytab_path: Some("/etc/krb5.keytab".to_string()),
            protection_level: "integrity".to_string(),
            verbose: false,
        };

        #[cfg(unix)]
        {
            let auth = GssApiAuthenticator::new(&settings);
            assert!(auth.is_ok());
        }

        #[cfg(not(unix))]
        {
            let auth = GssApiAuthenticator::new(&settings);
            assert!(auth.is_err());
        }
    }

    #[test]
    fn test_invalid_protection_level() {
        let settings = GssApiSettings {
            service_name: "socks@example.com".to_string(),
            keytab_path: None,
            protection_level: "invalid".to_string(),
            verbose: false,
        };

        #[cfg(unix)]
        {
            let auth = GssApiAuthenticator::new(&settings);
            assert!(auth.is_err());
        }
    }

    #[test]
    fn test_empty_service_name() {
        let settings = GssApiSettings {
            service_name: "".to_string(),
            keytab_path: None,
            protection_level: "integrity".to_string(),
            verbose: false,
        };

        #[cfg(unix)]
        {
            let auth = GssApiAuthenticator::new(&settings);
            assert!(auth.is_err());
        }
    }
}
