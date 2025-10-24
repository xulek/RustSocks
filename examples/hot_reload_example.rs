/// Example: ACL Hot Reload
///
/// This example demonstrates how to use the ACL hot reload feature.
/// The watcher will automatically reload the ACL configuration when the file changes.
use rustsocks::acl::{load_acl_config_sync, AclEngine, AclWatcher};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Path to ACL config
    let config_path = PathBuf::from("config/acl.toml");

    // Load initial ACL configuration
    let acl_config = load_acl_config_sync(&config_path)?;
    let engine = Arc::new(AclEngine::new(acl_config)?);

    // Create and start the watcher
    let mut watcher = AclWatcher::new(config_path.clone(), engine.clone());
    watcher.start().await?;

    println!("ACL hot reload enabled. Watching: {:?}", config_path);
    println!("Try modifying the config file - changes will be automatically applied!");
    println!("Press Ctrl+C to exit.");

    // Keep running
    loop {
        sleep(Duration::from_secs(1)).await;

        // You can query the engine while it's being watched
        let user_count = engine.get_user_count().await;
        let group_count = engine.get_group_count().await;

        println!(
            "Current ACL state: {} users, {} groups",
            user_count, group_count
        );
    }
}
