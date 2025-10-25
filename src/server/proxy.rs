use crate::session::SessionManager;
use crate::utils::error::{Result, RustSocksError};
use std::io;
use std::num::NonZeroU64;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace};
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
        Self::new(10)
    }
}

#[derive(Debug, Clone, Copy)]
enum TrafficDirection {
    Upload,
    Download,
}

#[derive(Debug, Default)]
struct TrafficTotals {
    bytes: u64,
    packets: u64,
}

/// Proxy data bidirectionally between client and upstream server while tracking traffic.
pub async fn proxy_data(
    client: TcpStream,
    upstream: TcpStream,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    cancel_token: CancellationToken,
    update_config: TrafficUpdateConfig,
) -> Result<()> {
    let (client_read, client_write) = client.into_split();
    let (upstream_read, upstream_write) = upstream.into_split();

    let upload_handle = tokio::spawn(proxy_direction(
        client_read,
        upstream_write,
        session_manager.clone(),
        session_id,
        cancel_token.clone(),
        update_config,
        TrafficDirection::Upload,
    ));

    let download_handle = tokio::spawn(proxy_direction(
        upstream_read,
        client_write,
        session_manager,
        session_id,
        cancel_token,
        update_config,
        TrafficDirection::Download,
    ));

    let (upload_result, download_result) = tokio::join!(upload_handle, download_handle);

    let upload = upload_result.map_err(join_error_to_rustsocks)?;
    let download = download_result.map_err(join_error_to_rustsocks)?;

    match (upload, download) {
        (Ok(up), Ok(down)) => {
            debug!(
                "Proxy completed: {} bytes ↑ ({} packets), {} bytes ↓ ({} packets)",
                up.bytes, up.packets, down.bytes, down.packets
            );
            Ok(())
        }
        (Err(err), Ok(_)) | (Ok(_), Err(err)) => {
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

async fn proxy_direction(
    mut reader: OwnedReadHalf,
    mut writer: OwnedWriteHalf,
    session_manager: Arc<SessionManager>,
    session_id: Uuid,
    cancel_token: CancellationToken,
    update_config: TrafficUpdateConfig,
    direction: TrafficDirection,
) -> Result<TrafficTotals> {
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut totals = TrafficTotals::default();
    let mut pending_bytes = 0u64;
    let mut pending_packets = 0u64;
    let packet_interval = update_config.packet_interval().get();
    let mut cancelled = false;

    loop {
        let read_result = tokio::select! {
            _ = cancel_token.cancelled() => {
                trace!("Direction {:?} cancelled", direction);
                cancelled = true;
                break;
            }
            result = reader.read(&mut buffer) => result,
        };

        let bytes_read = match read_result {
            Ok(0) => {
                trace!("Direction {:?} reached EOF", direction);
                break;
            }
            Ok(n) => n,
            Err(e) => {
                error!("Read error on {:?}: {}", direction, e);
                if pending_packets > 0 {
                    flush_pending(
                        &session_manager,
                        &session_id,
                        direction,
                        &mut pending_bytes,
                        &mut pending_packets,
                    )
                    .await;
                }
                return Err(RustSocksError::Io(e));
            }
        };

        if let Err(e) = writer.write_all(&buffer[..bytes_read]).await {
            error!("Write error on {:?}: {}", direction, e);
            if pending_packets > 0 {
                flush_pending(
                    &session_manager,
                    &session_id,
                    direction,
                    &mut pending_bytes,
                    &mut pending_packets,
                )
                .await;
            }
            return Err(RustSocksError::Io(e));
        }

        totals.bytes = totals.bytes.saturating_add(bytes_read as u64);
        totals.packets = totals.packets.saturating_add(1);
        pending_bytes = pending_bytes.saturating_add(bytes_read as u64);
        pending_packets = pending_packets.saturating_add(1);

        if pending_packets >= packet_interval {
            flush_pending(
                &session_manager,
                &session_id,
                direction,
                &mut pending_bytes,
                &mut pending_packets,
            )
            .await;
        }
    }

    if let Err(e) = writer.shutdown().await {
        error!("Shutdown error on {:?}: {}", direction, e);
        if pending_packets > 0 {
            flush_pending(
                &session_manager,
                &session_id,
                direction,
                &mut pending_bytes,
                &mut pending_packets,
            )
            .await;
        }
        return Err(RustSocksError::Io(e));
    }

    if pending_packets > 0 {
        flush_pending(
            &session_manager,
            &session_id,
            direction,
            &mut pending_bytes,
            &mut pending_packets,
        )
        .await;
    }

    if cancelled {
        return Err(RustSocksError::ConnectionClosed);
    }

    Ok(totals)
}

async fn flush_pending(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_config_defaults_to_one_when_zero() {
        let config = TrafficUpdateConfig::new(0);
        assert_eq!(config.packet_interval().get(), 1);
    }
}
