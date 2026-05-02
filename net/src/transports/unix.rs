use std::{
    io,
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};

use karyon_core::async_runtime::{
    io::{AsyncRead, AsyncWrite},
    net::{UnixListener as AsyncUnixListener, UnixStream},
};

#[cfg(feature = "tokio")]
use tokio::io::ReadBuf;

use crate::{ByteStream, Endpoint, Error, Result};

/// Unix stream implementing `ByteStream`.
pub struct UnixByteStream {
    inner: UnixStream,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl ByteStream for UnixByteStream {
    fn peer_endpoint(&self) -> Option<Endpoint> {
        self.peer_endpoint.clone()
    }
    fn local_endpoint(&self) -> Option<Endpoint> {
        self.local_endpoint.clone()
    }
}

/// Connect to a Unix socket.
pub async fn connect(endpoint: &Endpoint) -> Result<Box<dyn ByteStream>> {
    let path: PathBuf = endpoint.clone().try_into()?;
    let stream = UnixStream::connect(&path).await?;
    let peer = stream
        .peer_addr()
        .ok()
        .and_then(|a| a.as_pathname().map(Endpoint::new_unix_addr));
    let local = stream
        .local_addr()
        .ok()
        .and_then(|a| a.as_pathname().map(Endpoint::new_unix_addr));
    Ok(Box::new(UnixByteStream {
        inner: stream,
        peer_endpoint: peer,
        local_endpoint: local,
    }))
}

/// Unix socket listener.
pub struct UnixListener {
    inner: AsyncUnixListener,
}

impl UnixListener {
    /// Bind to a Unix socket path.
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        let path: PathBuf = endpoint.clone().try_into()?;
        let inner = AsyncUnixListener::bind(path)?;
        Ok(Self { inner })
    }

    /// Accept a new connection.
    pub async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        let (stream, _) = self.inner.accept().await?;
        let peer = stream
            .peer_addr()
            .ok()
            .and_then(|a| a.as_pathname().map(Endpoint::new_unix_addr));
        let local = stream
            .local_addr()
            .ok()
            .and_then(|a| a.as_pathname().map(Endpoint::new_unix_addr));
        Ok(Box::new(UnixByteStream {
            inner: stream,
            peer_endpoint: peer,
            local_endpoint: local,
        }))
    }

    /// Local endpoint.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        let addr = self.inner.local_addr()?;
        addr.as_pathname()
            .map(Endpoint::new_unix_addr)
            .ok_or_else(|| {
                Error::IO(io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "unnamed unix socket",
                ))
            })
    }
}

// -- AsyncRead / AsyncWrite delegation --

#[cfg(feature = "smol")]
impl AsyncRead for UnixByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "tokio")]
impl AsyncRead for UnixByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "smol")]
impl AsyncWrite for UnixByteStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

#[cfg(feature = "tokio")]
impl AsyncWrite for UnixByteStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
