use clap::Parser;
use rustsocks::config::Config;
use rustsocks::server::SocksServer;
use rustsocks::Result;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "RustSocks")]
#[command(about = "High-performance SOCKS5 proxy server in Rust", long_about = None)]
#[command(version)]
struct Args {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Bind address (overrides config)
    #[arg(long)]
    bind: Option<String>,

    /// Bind port (overrides config)
    #[arg(long)]
    port: Option<u16>,

    /// Generate example configuration file
    #[arg(long, value_name = "FILE")]
    generate_config: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle config generation
    if let Some(config_path) = args.generate_config {
        println!("Generating example configuration file: {:?}", config_path);
        Config::create_example(&config_path)?;
        println!("Example configuration file created successfully!");
        println!(
            "Edit the file and run: rustsocks --config {:?}",
            config_path
        );
        return Ok(());
    }

    // Initialize logging
    init_logging(&args.log_level)?;

    info!("RustSocks v{} starting", env!("CARGO_PKG_VERSION"));
    if let Ok(cwd) = std::env::current_dir() {
        info!("Current working directory: {}", cwd.display());
    }

    // Load configuration
    let mut config = if let Some(config_path) = args.config {
        info!("Loading configuration from: {:?}", config_path);
        Config::from_file(config_path)?
    } else {
        info!("No configuration file specified, using defaults");
        Config::default()
    };

    // Apply CLI overrides
    if let Some(bind) = args.bind {
        config.server.bind_address = bind;
    }
    if let Some(port) = args.port {
        config.server.bind_port = port;
    }

    // Create and run server
    let server = SocksServer::new(config).await?;

    info!("Server initialized, starting listener...");

    // Handle Ctrl+C for graceful shutdown
    let shutdown = tokio::spawn(async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl+C");
        info!("Received Ctrl+C, shutting down gracefully...");
    });

    // Run server
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Server error: {}", e);
                return Err(e);
            }
        }
        _ = shutdown => {
            info!("Server shutdown complete");
        }
    }

    server.shutdown().await;

    Ok(())
}

fn init_logging(level: &str) -> Result<()> {
    let env_filter = EnvFilter::try_new(level)
        .map_err(|e| rustsocks::RustSocksError::Config(format!("Invalid log level: {}", e)))?;

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer())
        .init();

    Ok(())
}
