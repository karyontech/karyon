use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use karyon_core::async_runtime::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener as AsyncTcpListener, TcpStream},
};

#[cfg(feature = "tokio")]
use tokio::io::ReadBuf;

use async_trait::async_trait;

use crate::{ByteStream, Endpoint, Result, transport::TransportListener};

use super::resolve::resolve_endpoint;

/// TCP config.
#[derive(Clone)]
pub struct TcpConfig {
    pub nodelay: bool,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self { nodelay: true }
    }
}

/// TCP stream implementing `ByteStream`.
pub struct TcpByteStream {
    inner: TcpStream,
    peer_endpoint: Endpoint,
    local_endpoint: Endpoint,
}

impl ByteStream for TcpByteStream {
    fn peer_endpoint(&self) -> Option<Endpoint> {
        Some(self.peer_endpoint.clone())
    }
    fn local_endpoint(&self) -> Option<Endpoint> {
        Some(self.local_endpoint.clone())
    }
}

/// Connect to a TCP endpoint.
pub async fn connect(endpoint: &Endpoint, config: TcpConfig) -> Result<Box<dyn ByteStream>> {
    let addr = resolve_endpoint(endpoint).await?;
    let socket = TcpStream::connect(addr).await?;
    socket.set_nodelay(config.nodelay)?;
    let peer = Endpoint::new_tcp_addr(socket.peer_addr()?);
    let local = Endpoint::new_tcp_addr(socket.local_addr()?);
    Ok(Box::new(TcpByteStream {
        inner: socket,
        peer_endpoint: peer,
        local_endpoint: local,
    }))
}

/// TCP listener. Accepts connections as `Box<dyn ByteStream>`.
pub struct TcpListener {
    inner: AsyncTcpListener,
    config: TcpConfig,
}

impl TcpListener {
    /// Bind to a TCP endpoint.
    pub async fn bind(endpoint: &Endpoint, config: TcpConfig) -> Result<Self> {
        let addr = SocketAddr::try_from(endpoint.clone())?;
        let inner = AsyncTcpListener::bind(addr).await?;
        Ok(Self { inner, config })
    }

    /// Accept a new connection.
    pub async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        let (socket, _) = self.inner.accept().await?;
        socket.set_nodelay(self.config.nodelay)?;
        let peer = Endpoint::new_tcp_addr(socket.peer_addr()?);
        let local = Endpoint::new_tcp_addr(socket.local_addr()?);
        Ok(Box::new(TcpByteStream {
            inner: socket,
            peer_endpoint: peer,
            local_endpoint: local,
        }))
    }

    /// Local endpoint this listener is bound to.
    pub fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tcp_addr(self.inner.local_addr()?))
    }
}

// A built-in listener is a transport listener too, so crates can hold it
// and a user transport's listener as one `Box<dyn TransportListener>`.
#[async_trait]
impl TransportListener for TcpListener {
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        TcpListener::accept(self).await
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        TcpListener::local_endpoint(self)
    }
}

// -- AsyncRead / AsyncWrite delegation --

#[cfg(feature = "smol")]
impl AsyncRead for TcpByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "tokio")]
impl AsyncRead for TcpByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

#[cfg(feature = "smol")]
impl AsyncWrite for TcpByteStream {
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
impl AsyncWrite for TcpByteStream {
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

#[cfg(test)]
mod tests {
    use karyon_core::testing::run_test;

    use super::*;

    #[test]
    fn tcp_listener_as_transport_listener() {
        run_test(10, async {
            let ep: Endpoint = "tcp://127.0.0.1:0".parse().unwrap();
            let listener = TcpListener::bind(&ep, TcpConfig::default()).await.unwrap();
            let listener: Box<dyn TransportListener> = Box::new(listener);

            let bound = listener.local_endpoint().unwrap();
            let client = connect(&bound, TcpConfig::default()).await.unwrap();
            let server = listener.accept().await.unwrap();

            assert_eq!(server.peer_endpoint(), client.local_endpoint());
            assert_eq!(server.local_endpoint(), Some(bound));
        });
    }
}
