use super::types::AclConfig;
use std::path::Path;
use tracing::info;

/// Load ACL configuration from TOML file
pub async fn load_acl_config<P: AsRef<Path>>(path: P) -> Result<AclConfig, String> {
    let content = tokio::fs::read_to_string(path.as_ref())
        .await
        .map_err(|e| format!("Failed to read ACL config file: {}", e))?;

    let config: AclConfig =
        toml::from_str(&content).map_err(|e| format!("Failed to parse ACL config: {}", e))?;

    // Validate
    config.validate()?;

    info!(
        users = config.users.len(),
        groups = config.groups.len(),
        "ACL configuration loaded successfully"
    );

    Ok(config)
}

/// Load ACL configuration synchronously (for blocking contexts)
pub fn load_acl_config_sync<P: AsRef<Path>>(path: P) -> Result<AclConfig, String> {
    let content = std::fs::read_to_string(path.as_ref())
        .map_err(|e| format!("Failed to read ACL config file: {}", e))?;

    let config: AclConfig =
        toml::from_str(&content).map_err(|e| format!("Failed to parse ACL config: {}", e))?;

    // Validate
    config.validate()?;

    info!(
        users = config.users.len(),
        groups = config.groups.len(),
        "ACL configuration loaded successfully"
    );

    Ok(config)
}

/// Create example ACL configuration file
pub fn create_example_acl_config<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let example = r#"# RustSocks ACL Configuration

[global]
default_policy = "block"  # Options: "allow", "block"

# Per-user ACL rules
[[users]]
username = "alice"
groups = ["developers", "ssh-users"]

  # BLOCK rules have highest priority
  [[users.rules]]
  action = "block"
  description = "Block access to admin panel"
  destinations = ["admin.company.com", "192.168.100.10"]
  ports = ["*"]
  protocols = ["both"]
  priority = 1000

  [[users.rules]]
  action = "allow"
  description = "Allow HTTPS to company network"
  destinations = ["10.0.0.0/8"]
  ports = ["443", "8000-9000"]
  protocols = ["tcp"]
  priority = 100

  [[users.rules]]
  action = "allow"
  description = "Allow access to production servers"
  destinations = ["prod-*.company.com", "192.168.100.0/24"]
  ports = ["443", "5432"]
  protocols = ["tcp"]
  priority = 100

[[users]]
username = "bob"
groups = ["readonly"]

  [[users.rules]]
  action = "allow"
  description = "Read-only database access"
  destinations = ["db-replica.company.com"]
  ports = ["5432"]
  protocols = ["tcp"]
  priority = 100

  [[users.rules]]
  action = "block"
  description = "Block write operations"
  destinations = ["db-master.company.com"]
  ports = ["*"]
  protocols = ["both"]
  priority = 1000

# Group rules (inherited by all users in group)
[[groups]]
name = "developers"

  [[groups.rules]]
  action = "allow"
  description = "Access to dev environments"
  destinations = ["*.dev.company.com", "10.1.0.0/16"]
  ports = ["*"]
  protocols = ["both"]
  priority = 50

[[groups]]
name = "ssh-users"

  [[groups.rules]]
  action = "allow"
  description = "SSH access"
  destinations = ["*"]
  ports = ["22"]
  protocols = ["tcp"]
  priority = 50

[[groups]]
name = "readonly"

  [[groups.rules]]
  action = "block"
  description = "Block SSH access"
  destinations = ["*"]
  ports = ["22"]
  protocols = ["tcp"]
  priority = 500
"#;

    std::fs::write(path.as_ref(), example)
        .map_err(|e| format!("Failed to write example ACL config: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_create_and_load_example() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create example
        create_example_acl_config(path).unwrap();

        // Load it
        let config = load_acl_config_sync(path).unwrap();

        // Verify
        assert_eq!(config.users.len(), 2);
        assert_eq!(config.groups.len(), 3);
        assert_eq!(config.users[0].username, "alice");
        assert_eq!(config.users[0].groups.len(), 2);
        assert_eq!(config.users[0].rules.len(), 3);
    }
}
