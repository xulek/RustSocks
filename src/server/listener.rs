use crate::acl::{load_acl_config_sync, AclEngine, AclStats, AclWatcher, PolicyUsage};
use crate::api::start_api_server;
use crate::api::types::ApiConfig;
use crate::auth::AuthManager;
use crate::config::{Config, TlsSettings};
use crate::qos::QosEngine;
use crate::server::handler::{handle_client, ClientHandlerContext};
use crate::server::net::tune_tcp_stream;
use crate::server::pool::ConnectionPool;
use crate::server::proxy::TrafficUpdateConfig;
use crate::session::{start_metrics_collector, MetricsHistory, SessionManager};
#[cfg(feature = "database")]
use crate::session::{BatchConfig, SessionStore};
#[cfg(feature = "database")]
use crate::smtp::notifications::{
    notify_with_store, resource_monitor_loop, NotificationDecision, NotificationKind,
};
use crate::telemetry::TelemetryHistory;
use crate::utils::error::{Result, RustSocksError};
use dashmap::DashMap;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use rustls::RootCertStore;
use std::ffi::OsString;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_rustls::{rustls, TlsAcceptor};
use tracing::{error, info, warn};

#[cfg(any(feature = "database", test))]
fn redact_database_url(url: &str) -> String {
    if url.starts_with("sqlite:") {
        return url.to_string();
    }

    let Some((scheme, rest)) = url.split_once("://") else {
        return "<redacted>".to_string();
    };

    let Some((credentials, suffix)) = rest.split_once('@') else {
        return format!("{scheme}://{rest}");
    };

    let username = credentials.split(':').next().unwrap_or("user");
    format!("{scheme}://{username}:***@{suffix}")
}

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

/// Global and optional per-address connection admission.
#[derive(Clone)]
struct ConnectionLimiter {
    semaphore: Arc<Semaphore>,
    per_ip: Option<Arc<PerIpLimiter>>,
}

/// Held for the lifetime of a connection; releases both the global slot and the
/// per-address slot on drop.
struct ConnectionPermit {
    _global: tokio::sync::OwnedSemaphorePermit,
    _per_ip: Option<IpPermit>,
}

impl ConnectionLimiter {
    fn new(max_connections: usize, max_per_ip: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_connections)),
            per_ip: (max_per_ip > 0).then(|| Arc::new(PerIpLimiter::new(max_per_ip))),
        }
    }

    fn try_acquire(&self, client: IpAddr) -> std::result::Result<ConnectionPermit, RejectReason> {
        let global = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| RejectReason::Global)?;
        let per_ip = match &self.per_ip {
            Some(limiter) => Some(limiter.try_acquire(client).ok_or(RejectReason::PerIp)?),
            None => None,
        };
        Ok(ConnectionPermit {
            _global: global,
            _per_ip: per_ip,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RejectReason {
    Global,
    PerIp,
}

struct PerIpLimiter {
    max_per_ip: usize,
    active: DashMap<IpAddr, usize>,
}

struct IpPermit {
    limiter: Arc<PerIpLimiter>,
    key: IpAddr,
}

impl PerIpLimiter {
    fn new(max_per_ip: usize) -> Self {
        Self {
            max_per_ip,
            active: DashMap::new(),
        }
    }

    /// IPv4-mapped IPv6 addresses count as IPv4; other IPv6 clients are grouped by /64 so a
    /// single subnet allocation cannot bypass the limit by rotating host bits.
    fn key(client: IpAddr) -> IpAddr {
        match client.to_canonical() {
            IpAddr::V6(v6) => {
                let mut octets = v6.octets();
                octets[8..].fill(0);
                IpAddr::V6(std::net::Ipv6Addr::from(octets))
            }
            v4 => v4,
        }
    }

    fn try_acquire(self: &Arc<Self>, client: IpAddr) -> Option<IpPermit> {
        let key = Self::key(client);
        let mut count = self.active.entry(key).or_insert(0);
        if *count >= self.max_per_ip {
            return None;
        }
        *count += 1;
        drop(count);
        Some(IpPermit {
            limiter: Arc::clone(self),
            key,
        })
    }
}

impl Drop for IpPermit {
    fn drop(&mut self) {
        if let dashmap::mapref::entry::Entry::Occupied(mut entry) =
            self.limiter.active.entry(self.key)
        {
            let remaining = entry.get().saturating_sub(1);
            if remaining == 0 {
                entry.remove();
            } else {
                *entry.get_mut() = remaining;
            }
        }
    }
}

/// Create a `TlsAcceptor` based on the server TLS settings.
pub fn create_tls_acceptor(tls: &TlsSettings) -> Result<TlsAcceptor> {
    if tls.key_password.is_some() {
        return Err(RustSocksError::Config(
            "server.tls.key_password is not supported (keys must be unencrypted)".to_string(),
        ));
    }

    let cert_path = tls.certificate_path.as_deref().ok_or_else(|| {
        RustSocksError::Config("certificate_path must be set when TLS is enabled".to_string())
    })?;
    let key_path = tls.private_key_path.as_deref().ok_or_else(|| {
        RustSocksError::Config("private_key_path must be set when TLS is enabled".to_string())
    })?;

    let certs = load_certificates(cert_path)?;
    let key = load_private_key(key_path)?;

    // Configure protocol versions in builder
    let protocol_versions: &[&'static rustls::SupportedProtocolVersion] =
        match tls.min_protocol_version.as_deref() {
            Some("TLS13") => &[&rustls::version::TLS13],
            _ => &[&rustls::version::TLS13, &rustls::version::TLS12],
        };

    let builder = rustls::ServerConfig::builder_with_protocol_versions(protocol_versions);

    let mut config = if tls.require_client_auth {
        let ca_path = tls.client_ca_path.as_deref().ok_or_else(|| {
            RustSocksError::Config(
                "client_ca_path must be set when client auth is enabled".to_string(),
            )
        })?;
        let root_store = build_client_root_store(ca_path)?;
        let client_verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| {
                RustSocksError::Config(format!("Failed to build client cert verifier: {}", e))
            })?;
        builder
            .with_client_cert_verifier(client_verifier)
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

    if !tls.alpn_protocols.is_empty() {
        config.alpn_protocols = tls
            .alpn_protocols
            .iter()
            .map(|proto| proto.as_bytes().to_vec())
            .collect();
    }

    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn load_certificates(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let path = Path::new(path);
    let certs: std::result::Result<Vec<_>, _> = CertificateDer::pem_file_iter(path)
        .map_err(|e| {
            RustSocksError::Config(format!(
                "Failed to open TLS certificate file '{}': {}",
                path.display(),
                e
            ))
        })?
        .collect();

    let certs = certs.map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to parse certificates from '{}': {}",
            path.display(),
            e
        ))
    })?;

    if certs.is_empty() {
        return Err(RustSocksError::Config(format!(
            "TLS certificate file '{}' did not contain any certificates",
            path.display()
        )));
    }

    Ok(certs)
}

fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let path = Path::new(path);
    PrivateKeyDer::from_pem_file(path).map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to load private key from '{}': {}",
            path.display(),
            e
        ))
    })
}

fn build_client_root_store(path: &str) -> Result<RootCertStore> {
    let path = Path::new(path);
    let certs: std::result::Result<Vec<_>, _> = CertificateDer::pem_file_iter(path)
        .map_err(|e| {
            RustSocksError::Config(format!(
                "Failed to open client CA file '{}': {}",
                path.display(),
                e
            ))
        })?
        .collect();

    let certs = certs.map_err(|e| {
        RustSocksError::Config(format!(
            "Failed to parse client CA certificates from '{}': {}",
            path.display(),
            e
        ))
    })?;

    if certs.is_empty() {
        return Err(RustSocksError::Config(format!(
            "Client CA file '{}' did not contain any certificates",
            path.display()
        )));
    }

    let mut store = RootCertStore::empty();
    let (added, _) = store.add_parsable_certificates(certs);
    if added == 0 {
        return Err(RustSocksError::Config(format!(
            "No valid client CA certificates could be loaded from '{}'",
            path.display()
        )));
    }

    Ok(store)
}

impl SocksServer {
    pub async fn new(
        config: Config,
        config_path: Option<PathBuf>,
        original_args: Arc<Vec<OsString>>,
    ) -> Result<Self> {
        crate::server::resolver::configure_resolver(&config.server.resolver);
        let auth_manager = Arc::new(AuthManager::new(&config.auth)?);

        let mut acl_engine: Option<Arc<AclEngine>> = None;
        let mut acl_watcher: Option<Mutex<AclWatcher>> = None;
        let mut watcher_setup: Option<(PathBuf, Arc<AclEngine>)> = None;

        if config.acl.enabled {
            let config_path_str = config.acl.config_file.as_ref().ok_or_else(|| {
                RustSocksError::Config(
                    "config_file must be provided when ACL is enabled".to_string(),
                )
            })?;

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
        let config_path_clone = config_path.clone();
        let original_args_clone = original_args.clone();

        let tls_acceptor = if config.server.tls.enabled {
            Some(create_tls_acceptor(&config.server.tls)?)
        } else {
            None
        };

        let policy_usage = PolicyUsage::connect(&config.policy_state).await?;
        info!(
            backend = policy_usage.backend_name(),
            "Policy usage state backend ready"
        );

        let session_manager_inner = SessionManager::new_with_policy_usage(
            config.sessions.traffic_queue_capacity,
            Some(Duration::from_secs(
                config.sessions.retention_days.saturating_mul(24 * 3600),
            )),
            config.sessions.history_max_entries,
            policy_usage,
        );

        #[cfg(feature = "database")]
        let mut session_manager_inner = session_manager_inner;

        #[cfg(feature = "database")]
        if config.sessions.enabled
            && matches!(
                config.sessions.storage.as_str(),
                "sqlite" | "mariadb" | "mysql"
            )
        {
            let url = config
                .sessions
                .database_url
                .as_ref()
                .ok_or_else(|| {
                    RustSocksError::Config(
                        "database_url must be provided when SQL-backed storage is enabled"
                            .to_string(),
                    )
                })?
                .clone();

            info!(
                database_url = %redact_database_url(&url),
                "Initializing session store"
            );

            match SessionStore::connect(&url).await {
                Ok(store) => {
                    // Mark all active sessions as closed (they can't still be running after restart)
                    if let Err(e) = store.close_all_active_sessions().await {
                        warn!(error = %e, "Failed to close stale active sessions on startup");
                    }

                    let arc_store = Arc::new(store);
                    let batch_config = BatchConfig::from_settings(
                        config.sessions.batch_size,
                        config.sessions.batch_interval_ms,
                        config.sessions.batch_queue_capacity,
                    );
                    session_manager_inner.set_store(arc_store.clone(), batch_config);
                    arc_store.spawn_cleanup(
                        config.sessions.retention_days,
                        config.sessions.cleanup_interval_hours,
                    );
                    info!("Session store initialized");
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
        let telemetry_history = if config.telemetry.enabled {
            info!(
                max_events = config.telemetry.max_events,
                retention_hours = config.telemetry.retention_hours,
                "Operational telemetry enabled"
            );
            Some(Arc::new(TelemetryHistory::new(
                config.telemetry.max_events,
                config.telemetry.retention_hours,
            )))
        } else {
            info!("Operational telemetry disabled");
            None
        };
        let connection_pool = Arc::new(ConnectionPool::new_with_telemetry(
            pool_config,
            telemetry_history.clone(),
        ));
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
                token: config.sessions.api_token.clone(),
                swagger_enabled: config.sessions.swagger_enabled,
                dashboard_enabled: config.sessions.dashboard_enabled,
                dashboard_auth: config.sessions.dashboard_auth.clone(),
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
                telemetry_history.clone(),
                config.clone(),
                config_path_clone.clone(),
                original_args_clone.clone(),
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
        let bind_ip = self
            .config
            .server
            .bind_address
            .parse::<std::net::IpAddr>()
            .map_err(|e| RustSocksError::Config(format!("Invalid server bind address: {e}")))?;
        let bind_addr = std::net::SocketAddr::new(bind_ip, self.config.server.bind_port);

        let listener = TcpListener::bind(bind_addr).await?;
        let limiter = ConnectionLimiter::new(
            self.config.server.max_connections,
            self.config.server.max_connections_per_ip,
        );

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
            handshake_timeout: Duration::from_millis(self.config.server.handshake_timeout_ms),
        });

        #[cfg(feature = "database")]
        if let Some(store) = self.session_manager.session_store() {
            let store = store.clone();
            let smtp_encryption_key = self.config.sessions.smtp_encryption_key.clone();
            let subject = "RustSocks service status: started".to_string();
            let body = format!(
                "RustSocks has started and is listening on {}.\n\nClient auth: {}\nSOCKS auth: {}\nACL enabled: {}\n",
                bind_addr,
                self.config.auth.client_method,
                self.config.auth.socks_method,
                if self.acl_engine.is_some() { "yes" } else { "no" }
            );
            tokio::spawn(async move {
                match notify_with_store(
                    &store,
                    smtp_encryption_key,
                    NotificationKind::ServiceStatus,
                    subject,
                    body,
                )
                .await
                {
                    Ok(NotificationDecision::Sent { recipients }) => {
                        info!(
                            "Service status notification sent to {} recipient(s)",
                            recipients
                        );
                    }
                    Ok(NotificationDecision::Skipped(_)) => {}
                    Err(err) => {
                        warn!("Failed to send service status notification: {}", err);
                    }
                }
            });
        }

        #[cfg(feature = "database")]
        if let Some(store) = self.session_manager.session_store() {
            let smtp_encryption_key = self.config.sessions.smtp_encryption_key.clone();
            let session_manager = self.session_manager.clone();
            let max_connections = self.config.server.max_connections;
            tokio::spawn(resource_monitor_loop(
                store,
                smtp_encryption_key,
                session_manager,
                max_connections,
                30,
            ));
        }

        let tls_acceptor = self.tls_acceptor.clone();

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New connection from {}", addr);

                    let permit = match limiter.try_acquire(addr.ip()) {
                        Ok(permit) => permit,
                        Err(RejectReason::Global) => {
                            warn!(
                                client = %addr,
                                max_connections = self.config.server.max_connections,
                                "Connection limit reached; dropping connection"
                            );
                            continue;
                        }
                        Err(RejectReason::PerIp) => {
                            warn!(
                                client = %addr,
                                max_connections_per_ip = self.config.server.max_connections_per_ip,
                                "Per-address connection limit reached; dropping connection"
                            );
                            continue;
                        }
                    };

                    // Optimize client TCP socket for low latency and throughput
                    tune_tcp_stream(&stream);

                    let ctx = handler_ctx.clone();
                    let tls_acceptor = tls_acceptor.clone();
                    let tls_handshake_timeout =
                        Duration::from_millis(self.config.server.tls.handshake_timeout_ms);

                    tokio::spawn(async move {
                        let _permit = permit;
                        let result = if let Some(acceptor) = tls_acceptor {
                            match timeout(tls_handshake_timeout, acceptor.accept(stream)).await {
                                Ok(Ok(tls_stream)) => handle_client(tls_stream, ctx, addr).await,
                                Ok(Err(e)) => {
                                    error!("TLS handshake failed for {}: {}", addr, e);
                                    return;
                                }
                                Err(_) => {
                                    warn!("TLS handshake timed out for {}", addr);
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

        self.qos_engine.shutdown().await;

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

#[cfg(test)]
mod tests {
    use super::{redact_database_url, ConnectionLimiter, RejectReason};
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn connection_limiter_enforces_capacity() {
        let limiter = ConnectionLimiter::new(2, 0);
        let client = ip("192.0.2.1");
        let permit1 = limiter.try_acquire(client);
        let permit2 = limiter.try_acquire(client);

        assert!(permit1.is_ok());
        assert!(permit2.is_ok());
        assert_eq!(
            limiter.try_acquire(client).err(),
            Some(RejectReason::Global)
        );

        drop(permit1);
        assert!(limiter.try_acquire(client).is_ok());
    }

    #[test]
    fn per_ip_limit_is_enforced_independently_and_released_on_drop() {
        let limiter = ConnectionLimiter::new(100, 2);
        let a = ip("192.0.2.1");
        let b = ip("192.0.2.2");

        let a1 = limiter.try_acquire(a).unwrap();
        let _a2 = limiter.try_acquire(a).unwrap();
        assert_eq!(limiter.try_acquire(a).err(), Some(RejectReason::PerIp));
        // A different address is unaffected.
        assert!(limiter.try_acquire(b).is_ok());

        drop(a1);
        assert!(limiter.try_acquire(a).is_ok());
    }

    #[test]
    fn per_ip_rejection_does_not_leak_global_slots() {
        let limiter = ConnectionLimiter::new(2, 1);
        let a = ip("192.0.2.1");
        let _held = limiter.try_acquire(a).unwrap();
        for _ in 0..10 {
            assert_eq!(limiter.try_acquire(a).err(), Some(RejectReason::PerIp));
        }
        // The one remaining global slot is still available to another address.
        assert!(limiter.try_acquire(ip("192.0.2.9")).is_ok());
    }

    #[test]
    fn ipv6_clients_are_grouped_by_slash_64_and_mapped_v4_counts_as_v4() {
        let limiter = ConnectionLimiter::new(100, 1);
        let _first = limiter.try_acquire(ip("2001:db8:1:2::1")).unwrap();
        // Same /64, different host bits.
        assert_eq!(
            limiter.try_acquire(ip("2001:db8:1:2:ffff::7")).err(),
            Some(RejectReason::PerIp)
        );
        // Different /64.
        assert!(limiter.try_acquire(ip("2001:db8:1:3::1")).is_ok());

        let _v4 = limiter.try_acquire(ip("198.51.100.5")).unwrap();
        assert_eq!(
            limiter.try_acquire(ip("::ffff:198.51.100.5")).err(),
            Some(RejectReason::PerIp)
        );
    }

    #[test]
    fn per_ip_table_does_not_retain_idle_addresses() {
        let limiter = ConnectionLimiter::new(100, 3);
        let permit = limiter.try_acquire(ip("192.0.2.1")).unwrap();
        drop(permit);
        assert_eq!(limiter.per_ip.as_ref().unwrap().active.len(), 0);
    }

    #[test]
    fn database_url_redaction_hides_passwords() {
        let redacted = redact_database_url("mysql://alice:secret@example.com:3306/rustsocks");
        assert_eq!(redacted, "mysql://alice:***@example.com:3306/rustsocks");
    }
}
