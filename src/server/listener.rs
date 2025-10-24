use crate::acl::{load_acl_config_sync, AclEngine, AclStats};
use crate::auth::AuthManager;
use crate::config::Config;
use crate::server::handler::handle_client;
use crate::utils::error::{Result, RustSocksError};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

pub struct SocksServer {
    config: Arc<Config>,
    auth_manager: Arc<AuthManager>,
    acl_engine: Option<Arc<AclEngine>>,
    acl_stats: Arc<AclStats>,
    anonymous_user: Arc<String>,
}

impl SocksServer {
    pub fn new(config: Config) -> Result<Self> {
        let auth_manager = Arc::new(AuthManager::new(&config.auth)?);

        let acl_engine = if config.acl.enabled {
            let config_path = config
                .acl
                .config_file
                .as_ref()
                .expect("validated: config_file must be provided when ACL is enabled");

            let acl_config = load_acl_config_sync(config_path).map_err(RustSocksError::Config)?;

            match AclEngine::new(acl_config) {
                Ok(engine) => {
                    info!("ACL engine initialized from {}", config_path);
                    Some(Arc::new(engine))
                }
                Err(e) => {
                    return Err(RustSocksError::Config(format!(
                        "Failed to initialize ACL engine: {}",
                        e
                    )));
                }
            }
        } else {
            info!("ACL engine disabled");
            None
        };

        let anonymous_user = Arc::new(config.acl.anonymous_user.clone());
        let config = Arc::new(config);

        Ok(Self {
            config,
            auth_manager,
            acl_engine,
            acl_stats: Arc::new(AclStats::default()),
            anonymous_user,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let bind_addr = format!(
            "{}:{}",
            self.config.server.bind_address, self.config.server.bind_port
        );

        let listener = TcpListener::bind(&bind_addr).await?;

        info!("RustSocks server listening on {}", bind_addr);
        info!("Authentication method: {}", self.config.auth.method);
        if self.acl_engine.is_some() {
            info!("ACL enforcement enabled");
        } else if self.config.acl.enabled {
            // Config may enable ACL but engine could fail to initialize earlier
            warn!("ACL configured as enabled but engine is unavailable");
        } else {
            info!("ACL enforcement disabled");
        }

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New connection from {}", addr);

                    let auth_manager = self.auth_manager.clone();
                    let acl_engine = self.acl_engine.clone();
                    let acl_stats = self.acl_stats.clone();
                    let anonymous_user = self.anonymous_user.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(
                            stream,
                            auth_manager,
                            acl_engine,
                            acl_stats,
                            anonymous_user,
                        )
                        .await
                        {
                            error!("Client error from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}
