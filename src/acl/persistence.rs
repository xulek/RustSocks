/// Atomic file persistence for ACL configuration
///
/// This module provides safe file operations with:
/// - Atomic writes (write to temp, then rename)
/// - Automatic backups before overwrite
/// - Rollback capability on errors

use super::types::AclConfig;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, error, info, warn};

/// Save ACL configuration to file atomically
///
/// Process:
/// 1. Create backup of existing file (if exists)
/// 2. Write new config to temporary file
/// 3. Validate config
/// 4. Atomically rename temp file to target file
/// 5. Delete backup on success
pub async fn save_config<P: AsRef<Path>>(
    config: &AclConfig,
    path: P,
) -> Result<(), String> {
    let path = path.as_ref();

    // 0. Validate config before saving
    config.validate()?;

    // 1. Create backup if file exists
    let backup_path = create_backup(path).await?;

    // 2. Serialize to TOML
    let toml_string = toml::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config to TOML: {}", e))?;

    // 3. Write to temporary file (same directory for atomic rename)
    let temp_path = get_temp_path(path);

    if let Err(e) = fs::write(&temp_path, &toml_string).await {
        // Restore backup if write failed
        if let Some(ref backup) = backup_path {
            let _ = restore_backup(backup, path).await;
        }
        return Err(format!("Failed to write temporary config file: {}", e));
    }

    debug!(path = ?temp_path, "Wrote ACL config to temporary file");

    // 4. Validate by loading it back
    match fs::read_to_string(&temp_path).await {
        Ok(content) => {
            if let Err(e) = toml::from_str::<AclConfig>(&content) {
                // Restore backup if validation failed
                let _ = fs::remove_file(&temp_path).await;
                if let Some(ref backup) = backup_path {
                    let _ = restore_backup(backup, path).await;
                }
                return Err(format!("Config validation failed: {}", e));
            }
        }
        Err(e) => {
            let _ = fs::remove_file(&temp_path).await;
            if let Some(ref backup) = backup_path {
                let _ = restore_backup(backup, path).await;
            }
            return Err(format!("Failed to read back temporary file: {}", e));
        }
    }

    // 5. Atomically rename temp to target (overwrites existing)
    if let Err(e) = fs::rename(&temp_path, path).await {
        // Restore backup if rename failed
        let _ = fs::remove_file(&temp_path).await;
        if let Some(ref backup) = backup_path {
            let _ = restore_backup(backup, path).await;
        }
        return Err(format!("Failed to rename temporary file: {}", e));
    }

    info!(path = ?path, "ACL config saved successfully");

    // 6. Clean up backup on success
    if let Some(backup) = backup_path {
        if let Err(e) = fs::remove_file(&backup).await {
            warn!(backup = ?backup, error = %e, "Failed to delete backup file");
        } else {
            debug!(backup = ?backup, "Deleted backup file");
        }
    }

    Ok(())
}

/// Create backup of existing file
///
/// Returns the backup path if a backup was created, None if original file didn't exist
async fn create_backup(path: &Path) -> Result<Option<PathBuf>, String> {
    // Check if original file exists
    if !path.exists() {
        debug!(path = ?path, "No existing file to backup");
        return Ok(None);
    }

    let backup_path = get_backup_path(path);

    match fs::copy(path, &backup_path).await {
        Ok(_) => {
            debug!(
                original = ?path,
                backup = ?backup_path,
                "Created backup of ACL config"
            );
            Ok(Some(backup_path))
        }
        Err(e) => {
            error!(
                original = ?path,
                backup = ?backup_path,
                error = %e,
                "Failed to create backup"
            );
            Err(format!("Failed to create backup: {}", e))
        }
    }
}

/// Restore from backup
async fn restore_backup(backup_path: &Path, target_path: &Path) -> Result<(), String> {
    match fs::copy(backup_path, target_path).await {
        Ok(_) => {
            warn!(
                backup = ?backup_path,
                target = ?target_path,
                "Restored ACL config from backup"
            );
            Ok(())
        }
        Err(e) => {
            error!(
                backup = ?backup_path,
                target = ?target_path,
                error = %e,
                "Failed to restore from backup"
            );
            Err(format!("Failed to restore from backup: {}", e))
        }
    }
}

/// Get backup file path (adds .backup extension)
fn get_backup_path(path: &Path) -> PathBuf {
    let mut backup = path.to_path_buf();
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("acl.toml");
    backup.set_file_name(format!("{}.backup", filename));
    backup
}

/// Get temporary file path (adds .tmp extension)
fn get_temp_path(path: &Path) -> PathBuf {
    let mut temp = path.to_path_buf();
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("acl.toml");

    // Use timestamp to avoid conflicts
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    temp.set_file_name(format!("{}.{}.tmp", filename, timestamp));
    temp
}

/// Load ACL configuration from file
pub async fn load_config<P: AsRef<Path>>(path: P) -> Result<AclConfig, String> {
    let path = path.as_ref();

    let content = fs::read_to_string(path)
        .await
        .map_err(|e| format!("Failed to read ACL config file: {}", e))?;

    let config: AclConfig = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse ACL config: {}", e))?;

    // Validate
    config.validate()?;

    debug!(path = ?path, "Loaded ACL config from file");

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::types::{Action, GlobalAclConfig};
    use tempfile::TempDir;

    fn create_test_config() -> AclConfig {
        AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![],
            groups: vec![],
        }
    }

    #[tokio::test]
    async fn test_save_and_load_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("acl.toml");

        let config = create_test_config();

        // Save
        assert!(save_config(&config, &config_path).await.is_ok());
        assert!(config_path.exists());

        // Load
        let loaded = load_config(&config_path).await.unwrap();
        assert_eq!(loaded.global.default_policy, Action::Block);
    }

    #[tokio::test]
    async fn test_backup_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("acl.toml");

        let config1 = create_test_config();
        save_config(&config1, &config_path).await.unwrap();

        // Save again - should create backup
        let mut config2 = create_test_config();
        config2.global.default_policy = Action::Allow;
        save_config(&config2, &config_path).await.unwrap();

        // Backup file should have been deleted on success
        let backup_path = get_backup_path(&config_path);
        assert!(!backup_path.exists());

        // Verify new config was saved
        let loaded = load_config(&config_path).await.unwrap();
        assert_eq!(loaded.global.default_policy, Action::Allow);
    }

    #[tokio::test]
    async fn test_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("acl.toml");

        let config = create_test_config();
        save_config(&config, &config_path).await.unwrap();

        // No temporary files should remain
        let temp_path = get_temp_path(&config_path);
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn test_validation_on_save() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("acl.toml");

        // Create invalid config (will be validated on save)
        let mut config = create_test_config();
        config.users.push(crate::acl::types::UserAcl {
            username: "alice".to_string(),
            groups: vec!["non-existent-group".to_string()],
            rules: vec![],
        });

        // Save should fail validation
        assert!(save_config(&config, &config_path).await.is_err());

        // File should not have been created
        assert!(!config_path.exists());
    }
}
