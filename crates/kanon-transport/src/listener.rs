//! Cross-platform IPC listener abstraction for Unix domain sockets and TCP loopback.
//!
//! Provides [`IpcListener`] and [`IpcIncoming`], which integrate seamlessly
//! with Tonic's gRPC server (`serve_with_incoming`).

use crate::IpcStream;
use crate::path::ensure_parent_dir;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A cross-platform listener for incoming IPC connections.
///
/// On Unix platforms, this listener wraps a [`tokio::net::UnixListener`].
/// On Windows platforms, this listener wraps a [`tokio::net::TcpListener`].
pub struct IpcListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    inner: tokio::net::TcpListener,
}

impl IpcListener {
    /// Binds an IPC listener to a Unix domain socket path.
    ///
    /// # Stale Socket Handling
    /// If an existing socket file already exists at the specified path (e.g. from an unclean
    /// termination of a previous instance), it is unlinked prior to binding to avoid `EADDRINUSE`.
    /// The parent directory is recursively created with secure permissions if it does not already exist.
    #[cfg(unix)]
    pub fn bind(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let path = path.as_ref();

        // Ensure parent directory exists before attempting to create the socket file.
        ensure_parent_dir(path)?;

        // Unlink any existing stale socket file to prevent Address Already In Use errors.
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        let inner = tokio::net::UnixListener::bind(path)?;
        tracing::debug!(path = %path.display(), "Bound IPC Unix listener successfully");
        Ok(Self { inner })
    }

    /// Binds an IPC listener to a TCP loopback address (typically used on Windows).
    #[cfg(windows)]
    pub fn bind(addr: std::net::SocketAddr) -> std::io::Result<Self> {
        let std_listener = std::net::TcpListener::bind(addr)?;
        std_listener.set_nonblocking(true)?;
        let inner = tokio::net::TcpListener::from_std(std_listener)?;
        tracing::debug!(addr = %addr, "Bound IPC TCP listener successfully");
        Ok(Self { inner })
    }

    /// Asynchronously accepts a new incoming IPC connection.
    pub async fn accept(&self) -> std::io::Result<IpcStream> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept().await?;
            Ok(IpcStream::new(stream))
        }
        #[cfg(windows)]
        {
            let (stream, _) = self.inner.accept().await?;
            Ok(IpcStream::new(stream))
        }
    }

    /// Converts this listener into an [`IpcIncoming`] stream for use with Tonic's
    /// `Server::serve_with_incoming`.
    pub fn incoming(self) -> IpcIncoming {
        IpcIncoming {
            #[cfg(unix)]
            inner: self.inner,
            #[cfg(windows)]
            inner: self.inner,
        }
    }
}

/// An incoming stream of [`IpcStream`] connections.
///
/// Implements [`futures_core::Stream`] so that it can be passed directly to
/// `tonic::transport::Server::serve_with_incoming`.
pub struct IpcIncoming {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    inner: tokio::net::TcpListener,
}

impl futures_core::Stream for IpcIncoming {
    type Item = std::io::Result<IpcStream>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        #[cfg(unix)]
        {
            match self.inner.poll_accept(cx) {
                Poll::Ready(Ok((stream, _))) => Poll::Ready(Some(Ok(IpcStream::new(stream)))),
                Poll::Ready(Err(err)) => Poll::Ready(Some(Err(err))),
                Poll::Pending => Poll::Pending,
            }
        }
        #[cfg(windows)]
        {
            match self.inner.poll_accept(cx) {
                Poll::Ready(Ok((stream, _))) => Poll::Ready(Some(Ok(IpcStream::new(stream)))),
                Poll::Ready(Err(err)) => Poll::Ready(Some(Err(err))),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}
