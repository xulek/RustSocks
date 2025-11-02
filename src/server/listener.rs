use crate::acl::{load_acl_config_sync, AclEngine, AclStats, AclWatcher};
use crate::api::start_api_server;
use crate::api::types::ApiConfig;
use crate::auth::AuthManager;
use crate::config::{Config, TlsSettings};
use crate::qos::QosEngine;
use crate::server::handler::{handle_client, ClientHandlerContext};
use crate::server::pool::ConnectionPool;
use crate::server::proxy::TrafficUpdateConfig;
use crate::session::{start_metrics_collector, MetricsHistory, SessionManager};
#[cfg(feature = "database")]
use crate::session::{BatchConfig, SessionStore};
use crate::utils::error::{Result, RustSocksError};
use rustls::server::AllowAnyAuthenticatedClient;
use rustls::{Certificate, PrivateKey, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_rustls::{rustls, TlsAcceptor};
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
    tls_acceptor: Option<TlsAcceptor>,
    connection_pool: Arc<ConnectionPool>,
}

/// Utwórz `TlsAcceptor` na podstawie ustawień TLS serwera.
pub fn create_tls_acceptor(tls: &TlsSettings) -> Result<TlsAcceptor> {
    if tls.key_password.is_some() {
        return Err(RustSocksError::Config(
            "server.tls.key_password is not supported (keys must be unencrypted)".to_string(),
        ));
    }

    let cert_path = tls
        .certificate_path
        .as_deref()
        .expect("validated: certificate_path must be set when TLS is enabled");
    let key_path = tls
        .private_key_path
        .as_deref()
        .expect("validated: private_key_path must be set when TLS is enabled");

    let certs = load_certificates(cert_path)?;
    let key = load_private_key(key_path)?;

    let protocol_versions: &[&'static rustls::SupportedProtocolVersion] =
        match tls.min_protocol_version.as_deref() {
            Some("TLS13") => &[&rustls::version::TLS13],
            _ => &[&rustls::version::TLS13, &rustls::version::TLS12],
        };

    let builder = rustls::ServerConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_protocol_versions(protocol_versions)
        .map_err(|e| {
            RustSocksError::Config(format!("Failed to configure TLS protocol versions: {}", e))
        })?;

    let server_config = if tls.require_client_auth {
        let ca_path = tls
            .client_ca_path
            .as_deref()
            .expect("validated: client_ca_path must be set when client auth is enabled");
        let root_store = build_client_root_store(ca_path)?;
        builder
            .with_client_cert_verifier(Arc::new(AllowAnyAuthenticatedClient::new(root_store)))
            .with_single_cert(certs, key)
            .map_err(|e| {
                RustSocksError::Config(format!("Failed to configure TLS certificates: {}", e))
            })?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| {
                RustSocksError::Config(format!("Failed to configure TLS certificates: {}", e))
            })?
    };

    let mut server_config = server_config;

    if !tls.alpn_protocols.is_empty() {
        server_config.alpn_protocols = tls
            .alpn_protocols
            .iter()
            .map(|proto| proto.as_bytes().to_vec())
            .collect();
    }

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

fn load_certificates(path: &str) -> Result<Vec<Certificate>> {
    let file = File::open(path).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to open TLS certificate file '{}': {}",
            path, e
        ))
    })?;
    let mut reader = BufReader::new(file);
    let certs = certs(&mut reader).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to parse certificates from '{}': {}",
            path, e
        ))
    })?;

    if certs.is_empty() {
        return Err(RustSocksError::Config(format!(
            "TLS certificate file '{}' did not contain any certificates",
            path
        )));
    }

    Ok(certs.into_iter().map(Certificate).collect())
}

fn load_private_key(path: &str) -> Result<PrivateKey> {
    let file = File::open(path).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to open TLS private key file '{}': {}",
            path, e
        ))
    })?;
    let mut reader = BufReader::new(file);
    let pkcs8_keys = pkcs8_private_keys(&mut reader).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to parse PKCS#8 private key from '{}': {}",
            path, e
        ))
    })?;
    if let Some(key) = pkcs8_keys.into_iter().next() {
        return Ok(PrivateKey(key));
    }

    let file = File::open(path).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to reopen TLS private key file '{}': {}",
            path, e
        ))
    })?;
    let mut reader = BufReader::new(file);
    let rsa_keys = rsa_private_keys(&mut reader).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to parse RSA private key from '{}': {}",
            path, e
        ))
    })?;
    if let Some(key) = rsa_keys.into_iter().next() {
        return Ok(PrivateKey(key));
    }

    Err(RustSocksError::Config(format!(
        "No supported private key found in '{}' (expected PKCS#8 or RSA)",
        path
    )))
}

fn build_client_root_store(path: &str) -> Result<RootCertStore> {
    let file = File::open(path).map_err(|e| {
        RustSocksError::Config(format!("Failed to open client CA file '{}': {}", path, e))
    })?;
    let mut reader = BufReader::new(file);
    let certs = certs(&mut reader).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to parse client CA certificates from '{}': {}",
            path, e
        ))
    })?;

    if certs.is_empty() {
        return Err(RustSocksError::Config(format!(
            "Client CA file '{}' did not contain any certificates",
            path
        )));
    }

    let mut store = RootCertStore::empty();
    let (added, _) = store.add_parsable_certificates(&certs);
    if added == 0 {
        return Err(RustSocksError::Config(format!(
            "No valid client CA certificates could be loaded from '{}'",
            path
        )));
    }

    Ok(store)
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

        let tls_acceptor = if config.server.tls.enabled {
            Some(create_tls_acceptor(&config.server.tls)?)
        } else {
            None
        };

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

        // Shared connection pool (used by proxy handlers and API telemetry)
        let pool_config = crate::server::pool::PoolConfig::from(config.server.pool.clone());
        let connection_pool = Arc::new(ConnectionPool::new(pool_config));
        if config.server.pool.enabled {
            info!(
                max_idle_per_dest = config.server.pool.max_idle_per_dest,
                max_total_idle = config.server.pool.max_total_idle,
                idle_timeout_secs = config.server.pool.idle_timeout_secs,
                "Connection pool enabled"
            );
        } else {
            info!("Connection pool disabled");
        }

        let mut stats_handle = None;

        if config.sessions.stats_api_enabled {
            let api_config = ApiConfig {
                bind_address: config.sessions.stats_api_bind_address.clone(),
                bind_port: config.sessions.stats_api_port,
                enable_api: true,
                token: None,
                swagger_enabled: config.sessions.swagger_enabled,
                dashboard_enabled: config.sessions.dashboard_enabled,
                base_path: config.sessions.normalized_base_path(),
            };

            let acl_config_path = if config.acl.enabled {
                config.acl.config_file.clone()
            } else {
                None
            };

            // Initialize metrics history based on config
            let metrics_history = if config.metrics.enabled {
                let max_snapshots = (config.metrics.retention_hours * 3600
                    / config.metrics.collection_interval_secs)
                    as usize;
                let max_age_hours = config.metrics.retention_hours as i64;

                let history = Arc::new(MetricsHistory::new(max_snapshots, max_age_hours));
                let history_clone = history.clone();
                let manager_clone = session_manager.clone();
                let collection_interval = config.metrics.collection_interval_secs;

                // Determine if we should persist to database
                #[cfg(feature = "database")]
                let use_database = config.metrics.storage == "sqlite"
                    && session_manager.as_ref().session_store().is_some();

                #[cfg(not(feature = "database"))]
                let _use_database = false;

                #[cfg(feature = "database")]
                let store_for_collector = if use_database {
                    session_manager.as_ref().session_store()
                } else {
                    None
                };

                #[cfg(feature = "database")]
                tokio::spawn(async move {
                    start_metrics_collector(
                        manager_clone,
                        history_clone,
                        store_for_collector,
                        collection_interval,
                    )
                    .await;
                });

                #[cfg(not(feature = "database"))]
                tokio::spawn(async move {
                    start_metrics_collector(manager_clone, history_clone, collection_interval)
                        .await;
                });

                // Start metrics cleanup task if using database
                #[cfg(feature = "database")]
                if use_database {
                    if let Some(store) = session_manager.as_ref().session_store() {
                        store.spawn_metrics_cleanup(
                            config.metrics.retention_hours,
                            config.metrics.cleanup_interval_hours,
                        );
                    }
                }

                info!(
                    storage = %config.metrics.storage,
                    retention_hours = config.metrics.retention_hours,
                    collection_interval_secs = config.metrics.collection_interval_secs,
                    "Metrics collection initialized"
                );

                Some(history)
            } else {
                info!("Metrics collection disabled");
                None
            };

            match start_api_server(
                api_config,
                session_manager.clone(),
                acl_engine.clone(),
                acl_config_path,
                connection_pool.clone(),
                metrics_history,
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
            tls_acceptor,
            connection_pool,
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
            connection_pool: self.connection_pool.clone(),
        });

        let tls_acceptor = self.tls_acceptor.clone();

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New connection from {}", addr);

                    // Optimize client TCP socket for low latency
                    if let Err(e) = stream.set_nodelay(true) {
                        warn!("Failed to set TCP_NODELAY on client socket: {}", e);
                    }

                    let ctx = handler_ctx.clone();
                    let tls_acceptor = tls_acceptor.clone();

                    tokio::spawn(async move {
                        let result = if let Some(acceptor) = tls_acceptor {
                            match acceptor.accept(stream).await {
                                Ok(tls_stream) => handle_client(tls_stream, ctx, addr).await,
                                Err(e) => {
                                    error!("TLS handshake failed for {}: {}", addr, e);
                                    return;
                                }
                            }
                        } else {
                            handle_client(stream, ctx, addr).await
                        };

                        if let Err(e) = result {
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
