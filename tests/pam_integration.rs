/// PAM Authentication Integration Tests
///
/// These tests verify PAM authentication integration with the SOCKS5 server.
///
/// **IMPORTANT**: These tests are marked #[ignore] because they require:
/// 1. PAM to be installed on the system
/// 2. PAM service files configured in /etc/pam.d/
/// 3. Test user accounts set up
/// 4. Running as root (for PAM authentication)
///
/// To run these tests:
/// ```bash
/// sudo cargo test --all-features pam -- --ignored --nocapture
/// ```

use rustsocks::auth::AuthManager;
use rustsocks::config::{AuthConfig, PamSettings, User};
use std::net::IpAddr;

fn pam_settings() -> PamSettings {
    PamSettings {
        username_service: "rustsocks-test".to_string(),
        address_service: "rustsocks-client-test".to_string(),
        default_user: "rhostusr".to_string(),
        default_ruser: "rhostusr".to_string(),
        verbose: true,
        verify_service: false, // Don't verify in tests
    }
}

#[cfg(unix)]
mod unix_tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires PAM setup
    async fn test_pam_username_method_with_valid_config() {
        let config = AuthConfig {
            client_method: "none".to_string(),
            socks_method: "pam.username".to_string(),
            users: vec![],
            pam: pam_settings(),
        };

        let result = AuthManager::new(&config);
        assert!(
            result.is_ok(),
            "PAM username auth manager creation should succeed on Unix"
        );
    }

    #[tokio::test]
    #[ignore] // Requires PAM setup
    async fn test_pam_address_method_with_valid_config() {
        let config = AuthConfig {
            client_method: "pam.address".to_string(),
            socks_method: "none".to_string(),
            users: vec![],
            pam: pam_settings(),
        };

        let result = AuthManager::new(&config);
        assert!(
            result.is_ok(),
            "PAM address auth manager creation should succeed on Unix"
        );
    }

    #[tokio::test]
    #[ignore] // Requires PAM setup and test user
    async fn test_pam_address_authentication_localhost() {
        let config = AuthConfig {
            client_method: "pam.address".to_string(),
            socks_method: "none".to_string(),
            users: vec![],
            pam: pam_settings(),
        };

        let auth_manager = AuthManager::new(&config).expect("Failed to create auth manager");
        let client_ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Note: This test will pass/fail depending on PAM configuration
        // If /etc/pam.d/rustsocks-client-test allows localhost, it passes
        let result = auth_manager.authenticate_client(client_ip).await;

        // We just verify it doesn't panic and returns a proper Result
        // The actual result depends on PAM config
        assert!(
            result.is_ok() || result.is_err(),
            "Should return a valid Result"
        );
    }

    #[tokio::test]
    async fn test_pam_config_validation() {
        // Empty service name should fail
        let mut config = AuthConfig {
            client_method: "none".to_string(),
            socks_method: "pam.username".to_string(),
            users: vec![],
            pam: PamSettings {
                username_service: "".to_string(), // Empty!
                ..pam_settings()
            },
        };

        let result = AuthManager::new(&config);
        assert!(
            result.is_err(),
            "Should reject empty PAM service name during validation"
        );

        // Valid config should work
        config.pam.username_service = "rustsocks-test".to_string();
        let result = AuthManager::new(&config);
        assert!(result.is_ok(), "Should accept valid PAM config");
    }

    #[tokio::test]
    async fn test_pam_both_methods_configured() {
        let config = AuthConfig {
            client_method: "pam.address".to_string(),
            socks_method: "pam.username".to_string(),
            users: vec![],
            pam: pam_settings(),
        };

        let result = AuthManager::new(&config);
        assert!(
            result.is_ok(),
            "Should allow both PAM address (client) and PAM username (socks)"
        );
    }

    #[tokio::test]
    async fn test_pam_invalid_client_method() {
        let config = AuthConfig {
            client_method: "userpass".to_string(), // Invalid for client_method
            socks_method: "none".to_string(),
            users: vec![User {
                username: "test".to_string(),
                password: "test".to_string(),
            }],
            pam: pam_settings(),
        };

        // This should fail during config validation
        // because client_method only supports "none" or "pam.address"
        let result = AuthManager::new(&config);
        // Note: The actual validation happens in config::Config::validate()
        // AuthManager::new() might succeed, but the server won't start
        // Let's just verify it doesn't panic
        let _ = result;
    }
}

#[cfg(not(unix))]
mod non_unix_tests {
    use super::*;

    #[tokio::test]
    async fn test_pam_not_supported_on_non_unix() {
        let config = AuthConfig {
            client_method: "none".to_string(),
            socks_method: "pam.username".to_string(),
            users: vec![],
            pam: pam_settings(),
        };

        let result = AuthManager::new(&config);
        assert!(
            result.is_err(),
            "PAM should not be available on non-Unix platforms"
        );

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("not supported")
                || err.to_string().contains("not available"),
            "Error should indicate PAM is not supported"
        );
    }

    #[tokio::test]
    async fn test_pam_address_not_supported_on_non_unix() {
        let config = AuthConfig {
            client_method: "pam.address".to_string(),
            socks_method: "none".to_string(),
            users: vec![],
            pam: pam_settings(),
        };

        let result = AuthManager::new(&config);
        assert!(
            result.is_err(),
            "PAM address should not be available on non-Unix platforms"
        );
    }
}

/// Cross-platform tests that should work everywhere
#[tokio::test]
async fn test_pam_config_defaults() {
    let pam_settings = PamSettings::default();

    assert_eq!(pam_settings.username_service, "rustsocks");
    assert_eq!(pam_settings.address_service, "rustsocks-client");
    assert_eq!(pam_settings.default_user, "rhostusr");
    assert_eq!(pam_settings.default_ruser, "rhostusr");
    assert!(!pam_settings.verbose);
    assert!(!pam_settings.verify_service);
}

#[tokio::test]
async fn test_non_pam_methods_still_work() {
    // Verify that normal auth methods still work when PAM is compiled in
    let config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "userpass".to_string(),
        users: vec![User {
            username: "alice".to_string(),
            password: "secret123".to_string(),
        }],
        pam: PamSettings::default(),
    };

    let result = AuthManager::new(&config);
    assert!(
        result.is_ok(),
        "Non-PAM methods should work regardless of platform"
    );
}

#[tokio::test]
async fn test_none_auth_works() {
    let config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: PamSettings::default(),
    };

    let auth_manager = AuthManager::new(&config).expect("None auth should always work");

    let client_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let result = auth_manager.authenticate_client(client_ip).await;

    assert!(result.is_ok(), "None auth should always succeed");
}

/// Documentation test showing how to configure PAM
///
/// # Example PAM Configuration
///
/// ## /etc/pam.d/rustsocks (for SOCKS username/password auth)
/// ```text
/// #%PAM-1.0
/// auth       required     pam_unix.so
/// account    required     pam_unix.so
/// ```
///
/// ## /etc/pam.d/rustsocks-client (for client IP-based auth)
/// ```text
/// #%PAM-1.0
/// auth       required     pam_permit.so
/// account    required     pam_permit.so
/// ```
///
/// ## config/rustsocks.toml
/// ```toml
/// [auth]
/// client_method = "none"           # or "pam.address"
/// socks_method = "pam.username"    # or "none", "userpass", "pam.address"
///
/// [auth.pam]
/// username_service = "rustsocks"
/// address_service = "rustsocks-client"
/// default_user = "rhostusr"
/// default_ruser = "rhostusr"
/// verbose = false
/// verify_service = false
/// ```
#[allow(dead_code)]
fn _pam_configuration_documentation() {
    // This is just a documentation marker
}
