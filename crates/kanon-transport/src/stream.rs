//! IPC client connection utilities.
//!
//! Provides helper functions to connect to local IPC endpoints (Unix domain sockets
//! or TCP loopback) and produce a Tonic [`tonic::transport::Channel`] for gRPC clients.

use std::path::PathBuf;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use crate::IpcStream;

/// Connects to a Unix domain socket at the specified path and returns a Tonic [`Channel`].
///
/// Under the hood, this configures a Tonic [`Endpoint`] with a custom connector function
/// that establishes a [`tokio::net::UnixStream`] connection wrapped in [`IpcStream`].
#[cfg(unix)]
pub async fn connect_unix(path: impl Into<PathBuf>) -> Result<Channel, tonic::transport::Error> {
    let path = path.into();
    // Tonic requires a valid URI for its internal HTTP/2 state machine,
    // even though the underlying transport is redirected to a Unix domain socket.
    Endpoint::try_from("http://localhost")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move {
                let stream = tokio::net::UnixStream::connect(path).await?;
                Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(IpcStream::new(stream)))
            }
        }))
        .await
}

/// Connects to a TCP loopback address and returns a Tonic [`Channel`].
#[cfg(windows)]
pub async fn connect_tcp(addr: std::net::SocketAddr) -> Result<Channel, tonic::transport::Error> {
    Endpoint::try_from(format!("http://{addr}"))?
        .connect_with_connector(service_fn(move |_: Uri| {
            async move {
                let stream = tokio::net::TcpStream::connect(addr).await?;
                Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(IpcStream::new(stream)))
            }
        }))
        .await
}

/// Connects to an IPC endpoint at the specified filesystem path and returns a Tonic [`Channel`].
///
/// On Unix platforms, this delegates directly to [`connect_unix`].
pub async fn connect_ipc(path: impl Into<PathBuf>) -> Result<Channel, tonic::transport::Error> {
    #[cfg(unix)]
    {
        connect_unix(path).await
    }
    #[cfg(windows)]
    {
        let path = path.into();
        let addr_str = std::fs::read_to_string(&path)
            .map_err(|e| tonic::transport::Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, e)))?;
        let addr: std::net::SocketAddr = addr_str
            .trim()
            .parse()
            .map_err(|e| tonic::transport::Error::from(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;
        connect_tcp(addr).await
    }
}
