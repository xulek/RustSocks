use crate::acl::{load_acl_config_sync, AclEngine, AclStats, AclWatcher};
use crate::api::start_api_server;
use crate::api::types::ApiConfig;
use crate::auth::AuthManager;
use crate::config::Config;
use crate::qos::QosEngine;
use crate::server::handler::{handle_client, ClientHandlerContext};
use crate::server::proxy::TrafficUpdateConfig;
use crate::session::SessionManager;
#[cfg(feature = "database")]
use crate::session::{BatchConfig, SessionStore};
use crate::utils::error::{Result, RustSocksError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
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
    acl_watcher: Option<Mutex<AclWatcher>>,
    qos_engine: QosEngine,
}

impl SocksServer {
    pub async fn new(config: Config) -> Result<Self> {
        let auth_manager = Arc::new(AuthManager::new(&config.auth)?);

        let mut acl_engine: Option<Arc<AclEngine>> = None;
        let mut acl_watcher: Option<Mutex<AclWatcher>> = None;
        let mut watcher_setup: Option<(PathBuf, Arc<AclEngine>)> = None;

        if config.acl.enabled {
            let config_path_str = config
                .acl
                .config_file
                .as_ref()
                .expect("validated: config_file must be provided when ACL is enabled");

            let config_path = Self::resolve_acl_path(config_path_str)?;

            let acl_config = load_acl_config_sync(&config_path).map_err(RustSocksError::Config)?;

            let engine = match AclEngine::new(acl_config) {
                Ok(engine) => {
                    info!("ACL engine initialized from {}", config_path.display());
                    Arc::new(engine)
                }
                Err(e) => {
                    return Err(RustSocksError::Config(format!(
                        "Failed to initialize ACL engine: {}",
                        e
                    )));
                }
            };

            if config.acl.watch {
                watcher_setup = Some((config_path.clone(), engine.clone()));
            }

            acl_engine = Some(engine);
        } else {
            info!("ACL engine disabled");
        }

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

            info!(database_url = %url, raw = ?url, "Initializing session store");

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

        if let Some((config_path, engine)) = watcher_setup {
            let mut watcher = AclWatcher::new(
                config_path.clone(),
                engine.clone(),
                Some(session_manager.clone()),
            );
            watcher.start().await.map_err(|e| {
                RustSocksError::Config(format!("Failed to start ACL watcher: {}", e))
            })?;

            info!(
                path = %config_path.display(),
                "ACL hot reload watcher enabled"
            );

            acl_watcher = Some(Mutex::new(watcher));
            acl_engine = Some(engine);
        }

        let traffic_config =
            TrafficUpdateConfig::new(config.sessions.traffic_update_packet_interval);

        let mut stats_handle = None;

        if config.sessions.stats_api_enabled {
            let api_config = ApiConfig {
                bind_address: config.sessions.stats_api_bind_address.clone(),
                bind_port: config.sessions.stats_api_port,
                enable_api: true,
                token: None,
            };

            let acl_config_path = if config.acl.enabled {
                config.acl.config_file.clone()
            } else {
                None
            };

            match start_api_server(
                api_config,
                session_manager.clone(),
                acl_engine.clone(),
                acl_config_path,
            )
            .await
            {
                Ok(handle) => {
                    stats_handle = Some(handle);
                }
                Err(e) => {
                    return Err(RustSocksError::Config(format!(
                        "Failed to start API server: {}",
                        e
                    )));
                }
            }
        }

        // Initialize QoS engine
        let qos_engine = QosEngine::from_config(config.qos.clone()).await?;
        if qos_engine.is_enabled() {
            info!("QoS engine initialized and started");
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
            acl_watcher,
            qos_engine,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let bind_addr = format!(
            "{}:{}",
            self.config.server.bind_address, self.config.server.bind_port
        );

        let listener = TcpListener::bind(&bind_addr).await?;

        info!("RustSocks server listening on {}", bind_addr);
        info!(
            "Authentication methods: client={}, socks={}",
            self.config.auth.client_method, self.config.auth.socks_method
        );
        if self.acl_engine.is_some() {
            info!("ACL enforcement enabled");
        } else if self.config.acl.enabled {
            // Config may enable ACL but engine could fail to initialize earlier
            warn!("ACL configured as enabled but engine is unavailable");
        } else {
            info!("ACL enforcement disabled");
        }

        let handler_ctx = Arc::new(ClientHandlerContext {
            auth_manager: self.auth_manager.clone(),
            acl_engine: self.acl_engine.clone(),
            acl_stats: self.acl_stats.clone(),
            anonymous_user: self.anonymous_user.clone(),
            session_manager: self.session_manager.clone(),
            traffic_config: self.traffic_config,
            qos_engine: self.qos_engine.clone(),
            connection_limits: self.config.qos.connection_limits.clone(),
        });

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New connection from {}", addr);

                    // Optimize client TCP socket for low latency
                    if let Err(e) = stream.set_nodelay(true) {
                        warn!("Failed to set TCP_NODELAY on client socket: {}", e);
                    }

                    let ctx = handler_ctx.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(stream, ctx, addr).await {
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
        if let Some(watcher) = &self.acl_watcher {
            let mut watcher = watcher.lock().await;
            watcher.stop();
        }

        if let Some(handle) = &self.stats_handle {
            handle.abort();
        }

        #[cfg(feature = "database")]
        self.session_manager.shutdown().await;
    }

    fn resolve_acl_path(path: &str) -> std::result::Result<PathBuf, RustSocksError> {
        let path_buf = PathBuf::from(path);
        let absolute_path = if path_buf.is_absolute() {
            path_buf
        } else {
            std::env::current_dir()
                .map_err(|e| {
                    RustSocksError::Config(format!(
                        "Failed to determine current directory for ACL config: {}",
                        e
                    ))
                })?
                .join(path_buf)
        };

        absolute_path.canonicalize().map_err(|e| {
            RustSocksError::Config(format!(
                "Failed to canonicalize ACL config path '{}': {}",
                path, e
            ))
        })
    }
}
