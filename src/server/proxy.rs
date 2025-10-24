use crate::utils::error::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{debug, error};

/// Proxy data bidirectionally between client and upstream server
pub async fn proxy_data(client: TcpStream, upstream: TcpStream) -> Result<()> {
    let (mut client_read, mut client_write) = client.into_split();
    let (mut upstream_read, mut upstream_write) = upstream.into_split();

    // Spawn two tasks: client->upstream and upstream->client
    let client_to_upstream = tokio::spawn(async move {
        match tokio::io::copy(&mut client_read, &mut upstream_write).await {
            Ok(bytes) => {
                debug!("Client->Upstream: {} bytes transferred", bytes);
                let _ = upstream_write.shutdown().await;
                Ok(bytes)
            }
            Err(e) => {
                error!("Client->Upstream error: {}", e);
                Err(e)
            }
        }
    });

    let upstream_to_client = tokio::spawn(async move {
        match tokio::io::copy(&mut upstream_read, &mut client_write).await {
            Ok(bytes) => {
                debug!("Upstream->Client: {} bytes transferred", bytes);
                let _ = client_write.shutdown().await;
                Ok(bytes)
            }
            Err(e) => {
                error!("Upstream->Client error: {}", e);
                Err(e)
            }
        }
    });

    // Wait for both directions to complete
    let (r1, r2) = tokio::join!(client_to_upstream, upstream_to_client);

    match (r1, r2) {
        (Ok(Ok(c2u)), Ok(Ok(u2c))) => {
            debug!("Proxy completed: {}↑ {}↓ bytes", c2u, u2c);
        }
        _ => {
            debug!("Proxy connection closed");
        }
    }

    Ok(())
}
