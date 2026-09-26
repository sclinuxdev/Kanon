//! Cross-platform unified IPC transport layer and security authentication abstraction for Kanon.
//!
//! Provides a seamless abstraction over Unix Domain Sockets (Linux/macOS) and
//! authenticated TCP loopback streams (Windows), ensuring compatibility with
//! the Tonic/Hyper gRPC ecosystem.

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tonic::transport::server::Connected;

pub mod listener;
pub mod path;
pub mod stream;

pub use listener::{IpcIncoming, IpcListener};
pub use path::{
    core_socket_path, default_run_dir, ensure_parent_dir, ensure_run_dir, host_socket_path,
};
pub use stream::connect_ipc;
#[cfg(windows)]
pub use stream::connect_tcp;
#[cfg(unix)]
pub use stream::connect_unix;

/// Header key used for Windows TCP loopback token-based authentication.
pub const AUTH_HEADER_KEY: &str = "x-kanon-auth-token";

/// Cross-platform IPC bidirectional byte stream abstraction.
///
/// Encapsulates `tokio::net::UnixStream` on Unix-like operating systems and
/// `tokio::net::TcpStream` on Windows. Implements standard Tokio async I/O traits
/// as well as Tonic's `Connected` trait for direct integration with `tonic::transport::Server`.
pub struct IpcStream {
    #[cfg(unix)]
    inner: tokio::net::UnixStream,
    #[cfg(windows)]
    inner: tokio::net::TcpStream,
}

impl IpcStream {
    /// Creates a new `IpcStream` wrapping an underlying Unix domain socket stream.
    #[cfg(unix)]
    pub fn new(inner: tokio::net::UnixStream) -> Self {
        Self { inner }
    }

    /// Creates a new `IpcStream` wrapping an underlying TCP loopback stream.
    #[cfg(windows)]
    pub fn new(inner: tokio::net::TcpStream) -> Self {
        Self { inner }
    }
}

impl AsyncRead for IpcStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for IpcStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl Connected for IpcStream {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

/// Tower/Tonic authentication interceptor for verifying IPC credentials.
///
/// On Windows, this interceptor verifies that incoming requests contain a valid
/// 32-byte authentication token in the `x-kanon-auth-token` metadata header using
/// constant-time comparison to prevent timing attacks.
/// On Unix, access is protected by POSIX filesystem permissions (0700).
#[derive(Clone)]
pub struct AuthInterceptor {
    #[allow(dead_code)]
    expected_token: String,
}

impl AuthInterceptor {
    /// Constructs a new `AuthInterceptor` with the expected token.
    pub fn new(token: String) -> Self {
        Self {
            expected_token: token,
        }
    }
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        #[cfg(windows)]
        {
            let token_header = request
                .metadata()
                .get(AUTH_HEADER_KEY)
                .and_then(|v| v.to_str().ok());

            match token_header {
                Some(token)
                    if constant_time_eq::constant_time_eq(
                        token.as_bytes(),
                        self.expected_token.as_bytes(),
                    ) =>
                {
                    Ok(request)
                }
                _ => Err(tonic::Status::unauthenticated(
                    "Invalid or missing IPC authentication token",
                )),
            }
        }

        #[cfg(unix)]
        {
            // On Unix platforms, access control is enforced via filesystem permissions (e.g. 0700 mode on socket directory).
            Ok(request)
        }
    }
}
