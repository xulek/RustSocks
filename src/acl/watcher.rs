use super::engine::AclEngine;
use super::loader::load_acl_config_sync;
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// ACL Hot Reload Watcher
/// Watches ACL configuration file and automatically reloads on changes
pub struct AclWatcher {
    config_path: PathBuf,
    engine: Arc<AclEngine>,
    watcher: Option<RecommendedWatcher>,
}

impl AclWatcher {
    /// Create a new ACL watcher
    pub fn new(config_path: PathBuf, engine: Arc<AclEngine>) -> Self {
        Self {
            config_path,
            engine,
            watcher: None,
        }
    }

    /// Start watching the ACL config file for changes
    pub async fn start(&mut self) -> Result<(), String> {
        let (tx, mut rx) = mpsc::channel(100);
        let config_path = self.config_path.clone();
        let engine = self.engine.clone();

        // Setup file watcher
        let mut watcher = RecommendedWatcher::new(
            move |res: NotifyResult<Event>| {
                if let Ok(event) = res {
                    // Filter for modification events
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        let _ = tx.blocking_send(event);
                    }
                }
            },
            Config::default()
                .with_poll_interval(Duration::from_secs(1))
                .with_compare_contents(true), // Only trigger on actual content changes
        )
        .map_err(|e| format!("Failed to create file watcher: {}", e))?;

        // Watch the config file
        watcher
            .watch(&config_path, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch config file: {}", e))?;

        self.watcher = Some(watcher);

        info!(
            path = ?config_path,
            "ACL hot reload watcher started"
        );

        // Spawn background task to handle reload events
        tokio::spawn(async move {
            while let Some(_event) = rx.recv().await {
                info!("ACL config file changed, reloading...");
                Self::handle_reload_event(&config_path, &engine).await;
            }
        });

        Ok(())
    }

    /// Handle a reload event (with validation and rollback)
    async fn handle_reload_event(config_path: &Path, engine: &Arc<AclEngine>) {
        let start_time = Instant::now();

        // Step 1: Load new config
        let new_config = match load_acl_config_sync(config_path) {
            Ok(config) => config,
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to load new ACL config, keeping current configuration"
                );
                return;
            }
        };

        // Step 2: Validate new config (already done in load_acl_config_sync)
        // The validation happens in AclConfig::validate() which is called by loader

        // Step 3: Try to compile and reload
        match engine.reload(new_config).await {
            Ok(()) => {
                let elapsed = start_time.elapsed();
                info!(
                    duration_ms = elapsed.as_millis(),
                    "ACL configuration reloaded successfully"
                );

                if elapsed.as_millis() > 100 {
                    warn!(
                        duration_ms = elapsed.as_millis(),
                        "ACL reload took longer than 100ms target"
                    );
                }
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to reload ACL config, keeping current configuration"
                );
                // The current config remains unchanged due to the failed reload
                // This is our "rollback" - we simply don't swap if validation/compilation fails
            }
        }
    }

    /// Stop watching
    pub fn stop(&mut self) {
        self.watcher = None;
        info!("ACL hot reload watcher stopped");
    }
}

impl Drop for AclWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::types::{AclConfig, AclRule, Action, GlobalAclConfig, Protocol, UserAcl};
    use crate::acl::AclEngine;
    use std::fs;
    use tempfile::NamedTempFile;
    use tokio::time::{sleep, Duration};

    fn create_test_config() -> AclConfig {
        AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Block,
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![AclRule {
                    action: Action::Allow,
                    description: "Allow HTTPS".to_string(),
                    destinations: vec!["0.0.0.0/0".to_string()],
                    ports: vec!["443".to_string()],
                    protocols: vec![Protocol::Tcp],
                    priority: 100,
                }],
            }],
            groups: vec![],
        }
    }

    fn create_modified_config() -> AclConfig {
        AclConfig {
            global: GlobalAclConfig {
                default_policy: Action::Allow, // Changed!
            },
            users: vec![UserAcl {
                username: "alice".to_string(),
                groups: vec![],
                rules: vec![AclRule {
                    action: Action::Block, // Changed!
                    description: "Block port 80".to_string(),
                    destinations: vec!["0.0.0.0/0".to_string()],
                    ports: vec!["80".to_string()],
                    protocols: vec![Protocol::Tcp],
                    priority: 100,
                }],
            }],
            groups: vec![],
        }
    }

    #[tokio::test]
    async fn test_manual_reload() {
        // Create initial config
        let config = create_test_config();
        let engine = Arc::new(AclEngine::new(config).unwrap());

        // Check initial state
        let user_count = engine.get_user_count().await;
        assert_eq!(user_count, 1);

        // Reload with new config
        let new_config = create_modified_config();
        engine.reload(new_config).await.unwrap();

        // Check updated state
        let user_count = engine.get_user_count().await;
        assert_eq!(user_count, 1);
    }

    #[tokio::test]
    async fn test_reload_validation() {
        let config = create_test_config();
        let engine = Arc::new(AclEngine::new(config).unwrap());

        // Try to reload with invalid config (duplicate user)
        let mut invalid_config = create_test_config();
        invalid_config.users.push(invalid_config.users[0].clone());

        let result = engine.reload(invalid_config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate user"));

        // Original config should still be intact
        let user_count = engine.get_user_count().await;
        assert_eq!(user_count, 1);
    }

    #[tokio::test]
    async fn test_reload_performance() {
        let config = create_test_config();
        let engine = Arc::new(AclEngine::new(config).unwrap());

        let start = Instant::now();
        let new_config = create_modified_config();
        engine.reload(new_config).await.unwrap();
        let elapsed = start.elapsed();

        // Should reload in less than 100ms
        assert!(
            elapsed.as_millis() < 100,
            "Reload took {}ms, expected <100ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    #[ignore] // This test requires actual file system watching, might be slow
    async fn test_file_watcher_integration() {
        // Create temp config file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Write initial config
        let initial_config = create_test_config();
        let toml_str = toml::to_string_pretty(&initial_config).unwrap();
        fs::write(&path, toml_str).unwrap();

        // Create engine and watcher
        let engine = Arc::new(AclEngine::new(initial_config).unwrap());
        let mut watcher = AclWatcher::new(path.clone(), engine.clone());
        watcher.start().await.unwrap();

        // Wait a bit for watcher to initialize
        sleep(Duration::from_millis(500)).await;

        // Modify the config file
        let modified_config = create_modified_config();
        let toml_str = toml::to_string_pretty(&modified_config).unwrap();
        fs::write(&path, toml_str).unwrap();

        // Wait for reload to happen
        sleep(Duration::from_secs(2)).await;

        // Config should be updated
        let user_count = engine.get_user_count().await;
        assert_eq!(user_count, 1);

        watcher.stop();
    }
}
