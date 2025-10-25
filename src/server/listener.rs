use crate::acl::{load_acl_config_sync, AclEngine, AclStats};
use crate::auth::AuthManager;
use crate::config::Config;
use crate::server::handler::handle_client;
use crate::server::proxy::TrafficUpdateConfig;
use crate::server::stats;
use crate::session::SessionManager;
#[cfg(feature = "database")]
use crate::session::{BatchConfig, SessionStore};
use crate::utils::error::{Result, RustSocksError};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

pub struct SocksServer {
    config: Arc<Config>,
    auth_manager: Arc<AuthManager>,
    acl_engine: Option<Arc<AclEngine>>,
    acl_stats: Arc<AclStats>,
    anonymous_user: Arc<String>,
    session_manager: Arc<SessionManager>,
    traffic_config: TrafficUpdateConfig,
    stats_handle: Option<JoinHandle<()>>,
}

impl SocksServer {
    pub async fn new(config: Config) -> Result<Self> {
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

        #[cfg_attr(not(feature = "database"), allow(unused_mut))]
        let mut session_manager_inner = SessionManager::new();

        #[cfg(feature = "database")]
        if config.sessions.enabled && config.sessions.storage == "sqlite" {
            let url = config
                .sessions
                .database_url
                .as_ref()
                .expect("validated: database_url present when sqlite storage enabled")
                .clone();

            match SessionStore::connect(&url).await {
                Ok(store) => {
                    let arc_store = Arc::new(store);
                    let batch_config = BatchConfig::from_settings(
                        config.sessions.batch_size,
                        config.sessions.batch_interval_ms,
                    );
                    session_manager_inner.set_store(arc_store.clone(), batch_config);
                    arc_store.spawn_cleanup(
                        config.sessions.retention_days,
                        config.sessions.cleanup_interval_hours,
                    );
                    info!("Session store initialized at {}", url);
                }
                Err(e) => {
                    return Err(RustSocksError::Config(format!(
                        "Failed to initialize session store: {}",
                        e
                    )));
                }
            }
        }

        let session_manager = Arc::new(session_manager_inner);

        let traffic_config =
            TrafficUpdateConfig::new(config.sessions.traffic_update_packet_interval);

        let mut stats_handle = None;

        if config.sessions.stats_api_enabled {
            let bind_addr = format!(
                "{}:{}",
                config.sessions.stats_api_bind_address, config.sessions.stats_api_port
            );
            let default_window_secs = config
                .sessions
                .stats_window_hours
                .checked_mul(3600)
                .ok_or_else(|| {
                    RustSocksError::Config("sessions.stats_window_hours is too large".to_string())
                })?;
            let default_window = Duration::from_secs(default_window_secs);

            match stats::start_stats_server(&bind_addr, session_manager.clone(), default_window)
                .await
            {
                Ok(handle) => {
                    stats_handle = Some(handle);
                }
                Err(e) => {
                    return Err(RustSocksError::Config(format!(
                        "Failed to start stats API: {}",
                        e
                    )));
                }
            }
        }

        Ok(Self {
            config,
            auth_manager,
            acl_engine,
            acl_stats: Arc::new(AclStats::default()),
            anonymous_user,
            session_manager,
            traffic_config,
            stats_handle,
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
                    let session_manager = self.session_manager.clone();
                    let traffic_config = self.traffic_config;

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(
                            stream,
                            auth_manager,
                            acl_engine,
                            acl_stats,
                            anonymous_user,
                            session_manager,
                            traffic_config,
                            addr,
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

    pub async fn shutdown(&self) {
        if let Some(handle) = &self.stats_handle {
            handle.abort();
        }

        #[cfg(feature = "database")]
        self.session_manager.shutdown().await;
    }
}
