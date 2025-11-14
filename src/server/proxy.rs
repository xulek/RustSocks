use crate::qos::{QosEngine, QosMetrics};
use crate::server::pool::ReuseHint;
use crate::session::SessionManager;
use crate::utils::error::{Result, RustSocksError};
use std::io;
use std::io::ErrorKind;
use std::num::NonZeroU64;
use std::sync::Arc;
use tokio::io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf, ReuniteError};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, instrument, trace};
use uuid::Uuid;

// Increased from 16KB to 32KB for better throughput on large transfers
// Reduces syscalls by 50% for large file transfers
const BUFFER_SIZE: usize = 32 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct TrafficUpdateConfig {
    packet_interval: NonZeroU64,
}

impl TrafficUpdateConfig {
    pub fn new(packet_interval: u64) -> Self {
        let fallback = NonZeroU64::new(1).expect("1 is non-zero");
        let packet_interval = NonZeroU64::new(packet_interval).unwrap_or(fallback);
        Self { packet_interval }
    }

    pub fn packet_interval(&self) -> NonZeroU64 {
        self.packet_interval
    }
}

impl Default for TrafficUpdateConfig {
    fn default() -> Self {
        // Increased from 10 to 50 packets for performance optimization
        // Reduces session manager lock contention by 5x at high concurrency
        // Tradeoff: Slightly delayed traffic statistics (50-500KB vs 10-100KB granularity)
        Self::new(50)
    }
}

#[derive(Debug, Clone, Copy)]
enum TrafficDirection {
    Upload,
    Download,
}

impl TrafficDirection {
    fn metric_label(&self) -> &'static str {
        match self {
            TrafficDirection::Upload => "upload",
            TrafficDirection::Download => "download",
        }
    }
}

#[derive(Debug, Default)]
struct TrafficTotals {
    bytes: u64,
    packets: u64,
}

struct UploadResult {
    totals: TrafficTotals,
    write_half: OwnedWriteHalf,
    client_closed: bool,
}

struct DownloadResult<W> {
    totals: TrafficTotals,
    read_half: OwnedReadHalf,
    remote_closed: bool,
    _writer: W,
}

/// Result of proxying that provides an upstream stream together with reuse guidance.
pub struct UpstreamReuse {
    pub stream: TcpStream,
    pub hint: ReuseHint,
}

/// Proxy data bidirectionally between client and upstream server while tracking traffic.
#[allow(clippy::too_many_arguments)]
#[instrument(
    level = "debug",
    skip(client, upstream, session_manager, cancel_token, qos_engine, user),
    fields(session = %session_id, user = %user)
)]
pub async fn proxy_data<S>(
    client: S,
    upstream: TcpStream,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    cancel_token: CancellationToken,
    update_config: TrafficUpdateConfig,
    qos_engine: QosEngine,
    user: Arc<str>,
) -> Result<Option<UpstreamReuse>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (client_read, client_write) = split(client);
    let (upstream_read, upstream_write) = upstream.into_split();

    let upload_handle = tokio::spawn(proxy_upload(
        client_read,
        upstream_write,
        session_manager.clone(),
        session_id,
        cancel_token.clone(),
        update_config,
        qos_engine.clone(),
        Arc::clone(&user),
    ));

    let download_handle = tokio::spawn(proxy_download(
        upstream_read,
        client_write,
        session_manager,
        session_id,
        cancel_token,
        update_config,
        qos_engine,
        user,
    ));

    let (upload_result, download_result) = tokio::join!(upload_handle, download_handle);

    let upload = upload_result.map_err(join_error_to_rustsocks)?;
    let download = download_result.map_err(join_error_to_rustsocks)?;

    match (upload, download) {
        (Ok(up), Ok(down)) => {
            let UploadResult {
                totals: up_totals,
                write_half,
                client_closed,
            } = up;
            let DownloadResult {
                totals: down_totals,
                read_half,
                remote_closed,
                ..
            } = down;

            debug!(
                "Proxy completed: {} bytes ↑ ({} packets), {} bytes ↓ ({} packets)",
                up_totals.bytes, up_totals.packets, down_totals.bytes, down_totals.packets
            );

            match read_half.reunite(write_half) {
                Ok(mut stream) => {
                    let hint = if client_closed
                        && !remote_closed
                        && up_totals.bytes == 0
                        && down_totals.bytes == 0
                    {
                        ReuseHint::Reuse
                    } else {
                        ReuseHint::Refresh
                    };

                    if matches!(hint, ReuseHint::Refresh) {
                        if let Err(e) = stream.shutdown().await {
                            trace!("Failed to shutdown upstream stream before refresh: {}", e);
                        }
                    }

                    Ok(Some(UpstreamReuse { stream, hint }))
                }
                Err(ReuniteError(read, write)) => {
                    drop(read);
                    drop(write);
                    Ok(None)
                }
            }
        }
        (Err(err), Ok(_down)) => {
            if matches!(err, RustSocksError::ConnectionClosed) {
                Err(RustSocksError::ConnectionClosed)
            } else {
                Err(err)
            }
        }
        (Ok(up), Err(err)) => {
            let UploadResult { write_half, .. } = up;
            drop(write_half);
            if matches!(err, RustSocksError::ConnectionClosed) {
                Err(RustSocksError::ConnectionClosed)
            } else {
                Err(err)
            }
        }
        (Err(err1), Err(err2)) => {
            if matches!(err1, RustSocksError::ConnectionClosed)
                || matches!(err2, RustSocksError::ConnectionClosed)
            {
                Err(RustSocksError::ConnectionClosed)
            } else {
                Err(err1)
            }
        }
    }
}

fn join_error_to_rustsocks(err: tokio::task::JoinError) -> RustSocksError {
    RustSocksError::Io(io::Error::other(format!("proxy task join error: {}", err)))
}

fn is_connection_closed_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected
            | ErrorKind::UnexpectedEof
    )
}

#[allow(clippy::too_many_arguments)]
#[instrument(
    level = "trace",
    skip(
        reader,
        upstream_write,
        session_manager,
        cancel_token,
        qos_engine,
        user
    )
)]
async fn proxy_upload<R>(
    mut reader: R,
    mut upstream_write: OwnedWriteHalf,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    cancel_token: CancellationToken,
    update_config: TrafficUpdateConfig,
    qos_engine: QosEngine,
    user: Arc<str>,
) -> Result<UploadResult>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut totals = TrafficTotals::default();
    let mut pending_bytes = 0u64;
    let mut pending_packets = 0u64;
    let packet_interval = update_config.packet_interval().get();
    let mut cancelled = false;
    let mut client_closed = false;

    loop {
        let read_result = tokio::select! {
            _ = cancel_token.cancelled() => {
                trace!("Direction {:?} cancelled", TrafficDirection::Upload);
                cancelled = true;
                break;
            }
            result = reader.read(&mut buffer) => result,
        };

        let bytes_read = match read_result {
            Ok(0) => {
                trace!("Direction {:?} reached EOF", TrafficDirection::Upload);
                client_closed = true;
                break;
            }
            Ok(n) => n,
            Err(e) => {
                if is_connection_closed_error(&e) {
                    trace!(
                        "Upload stream closed with error {:?}, treating as EOF",
                        e.kind()
                    );
                    client_closed = true;
                    break;
                } else {
                    error!("Read error on {:?}: {}", TrafficDirection::Upload, e);
                    if pending_packets > 0 {
                        flush_pending_now(
                            &session_manager,
                            &session_id,
                            TrafficDirection::Upload,
                            &mut pending_bytes,
                            &mut pending_packets,
                        )
                        .await;
                    }
                    return Err(RustSocksError::Io(e));
                }
            }
        };

        qos_engine
            .allocate_bandwidth_arc(&user, bytes_read as u64)
            .await?;
        if bytes_read > 0 {
            QosMetrics::record_allocation(
                user.as_ref(),
                TrafficDirection::Upload.metric_label(),
                bytes_read as u64,
            );
        }

        if let Err(e) = upstream_write.write_all(&buffer[..bytes_read]).await {
            if is_connection_closed_error(&e) {
                trace!(
                    "Upload write closed with error {:?}, treating as EOF",
                    e.kind()
                );
                client_closed = true;
                break;
            } else {
                error!("Write error on {:?}: {}", TrafficDirection::Upload, e);
                if pending_packets > 0 {
                    flush_pending_now(
                        &session_manager,
                        &session_id,
                        TrafficDirection::Upload,
                        &mut pending_bytes,
                        &mut pending_packets,
                    )
                    .await;
                }
                return Err(RustSocksError::Io(e));
            }
        }

        totals.bytes = totals.bytes.saturating_add(bytes_read as u64);
        totals.packets = totals.packets.saturating_add(1);
        pending_bytes = pending_bytes.saturating_add(bytes_read as u64);
        pending_packets = pending_packets.saturating_add(1);

        if pending_packets >= packet_interval {
            enqueue_pending_update(
                &session_manager,
                &session_id,
                TrafficDirection::Upload,
                &mut pending_bytes,
                &mut pending_packets,
            )
            .await;
        }
    }

    if pending_packets > 0 {
        flush_pending_now(
            &session_manager,
            &session_id,
            TrafficDirection::Upload,
            &mut pending_bytes,
            &mut pending_packets,
        )
        .await;
    }

    cancel_token.cancel();

    if cancelled {
        Err(RustSocksError::ConnectionClosed)
    } else {
        Ok(UploadResult {
            totals,
            write_half: upstream_write,
            client_closed,
        })
    }
}

#[allow(clippy::too_many_arguments)]
#[instrument(
    level = "trace",
    skip(upstream_read, writer, session_manager, cancel_token, qos_engine, user)
)]
async fn proxy_download<W>(
    mut upstream_read: OwnedReadHalf,
    mut writer: W,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    cancel_token: CancellationToken,
    update_config: TrafficUpdateConfig,
    qos_engine: QosEngine,
    user: Arc<str>,
) -> Result<DownloadResult<W>>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut totals = TrafficTotals::default();
    let mut pending_bytes = 0u64;
    let mut pending_packets = 0u64;
    let packet_interval = update_config.packet_interval().get();
    let mut cancelled = false;
    let mut remote_closed = false;

    loop {
        let read_result = tokio::select! {
            _ = cancel_token.cancelled() => {
                trace!("Direction {:?} cancelled", TrafficDirection::Download);
                cancelled = true;
                break;
            }
            result = upstream_read.read(&mut buffer) => result,
        };

        let bytes_read = match read_result {
            Ok(0) => {
                trace!("Direction {:?} reached EOF", TrafficDirection::Download);
                remote_closed = true;
                break;
            }
            Ok(n) => n,
            Err(e) => {
                if is_connection_closed_error(&e) {
                    trace!(
                        "Download stream closed with error {:?}, treating as EOF",
                        e.kind()
                    );
                    remote_closed = true;
                    break;
                } else {
                    error!("Read error on {:?}: {}", TrafficDirection::Download, e);
                    if pending_packets > 0 {
                        flush_pending_now(
                            &session_manager,
                            &session_id,
                            TrafficDirection::Download,
                            &mut pending_bytes,
                            &mut pending_packets,
                        )
                        .await;
                    }
                    return Err(RustSocksError::Io(e));
                }
            }
        };

        qos_engine
            .allocate_bandwidth_arc(&user, bytes_read as u64)
            .await?;
        if bytes_read > 0 {
            QosMetrics::record_allocation(
                user.as_ref(),
                TrafficDirection::Download.metric_label(),
                bytes_read as u64,
            );
        }

        if let Err(e) = writer.write_all(&buffer[..bytes_read]).await {
            if is_connection_closed_error(&e) {
                trace!(
                    "Download write closed with error {:?}, treating as EOF",
                    e.kind()
                );
                remote_closed = true;
                break;
            } else {
                error!("Write error on {:?}: {}", TrafficDirection::Download, e);
                if pending_packets > 0 {
                    flush_pending_now(
                        &session_manager,
                        &session_id,
                        TrafficDirection::Download,
                        &mut pending_bytes,
                        &mut pending_packets,
                    )
                    .await;
                }
                return Err(RustSocksError::Io(e));
            }
        }

        totals.bytes = totals.bytes.saturating_add(bytes_read as u64);
        totals.packets = totals.packets.saturating_add(1);
        pending_bytes = pending_bytes.saturating_add(bytes_read as u64);
        pending_packets = pending_packets.saturating_add(1);

        if pending_packets >= packet_interval {
            enqueue_pending_update(
                &session_manager,
                &session_id,
                TrafficDirection::Download,
                &mut pending_bytes,
                &mut pending_packets,
            )
            .await;
        }
    }

    if pending_packets > 0 {
        flush_pending_now(
            &session_manager,
            &session_id,
            TrafficDirection::Download,
            &mut pending_bytes,
            &mut pending_packets,
        )
        .await;
    }

    cancel_token.cancel();

    if cancelled {
        Err(RustSocksError::ConnectionClosed)
    } else {
        Ok(DownloadResult {
            totals,
            read_half: upstream_read,
            remote_closed,
            _writer: writer,
        })
    }
}

async fn enqueue_pending_update(
    session_manager: &Arc<SessionManager>,
    session_id: &Uuid,
    direction: TrafficDirection,
    bytes: &mut u64,
    packets: &mut u64,
) {
    if *packets == 0 {
        return;
    }

    match direction {
        TrafficDirection::Upload => {
            session_manager
                .update_traffic(session_id, *bytes, 0, *packets, 0)
                .await;
        }
        TrafficDirection::Download => {
            session_manager
                .update_traffic(session_id, 0, *bytes, 0, *packets)
                .await;
        }
    }

    trace!(
        "Flushed {:?} traffic update: {} bytes / {} packets",
        direction,
        *bytes,
        *packets
    );

    *bytes = 0;
    *packets = 0;
}

async fn flush_pending_now(
    session_manager: &Arc<SessionManager>,
    session_id: &Uuid,
    direction: TrafficDirection,
    bytes: &mut u64,
    packets: &mut u64,
) {
    if *packets == 0 {
        return;
    }

    match direction {
        TrafficDirection::Upload => {
            session_manager
                .update_traffic(session_id, *bytes, 0, *packets, 0)
                .await;
        }
        TrafficDirection::Download => {
            session_manager
                .update_traffic(session_id, 0, *bytes, 0, *packets)
                .await;
        }
    }

    trace!(
        "Synchronously flushed {:?} traffic update: {} bytes / {} packets",
        direction,
        *bytes,
        *packets
    );

    *bytes = 0;
    *packets = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_config_defaults_to_one_when_zero() {
        let config = TrafficUpdateConfig::new(0);
        assert_eq!(config.packet_interval().get(), 1);
    }
}
