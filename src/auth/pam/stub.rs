use super::{PamAuthError, PamMethod};

pub struct PamAuthenticator;

impl PamAuthenticator {
    pub fn new(
        _method: PamMethod,
        _settings: &crate::config::PamSettings,
    ) -> Result<Self, PamAuthError> {
        Err(PamAuthError::NotSupported(
            "PAM is not available on this platform".to_string(),
        ))
    }

    pub async fn authenticate_address(
        &self,
        _client_ip: std::net::IpAddr,
    ) -> Result<(), PamAuthError> {
        Err(PamAuthError::NotSupported(
            "PAM is not available on this platform".to_string(),
        ))
    }

    pub async fn authenticate_username(
        &self,
        _client_ip: std::net::IpAddr,
        _username: &str,
        _password: &str,
    ) -> Result<(), PamAuthError> {
        Err(PamAuthError::NotSupported(
            "PAM is not available on this platform".to_string(),
        ))
    }
}
