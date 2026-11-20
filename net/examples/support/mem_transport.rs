//! A transport for `mem://` endpoints: in-memory pipes instead of OS
//! sockets, so it works on WASI and on any executor. Shared by
//! `examples/custom_transport.rs` and `tests/custom_transport.rs`.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf, duplex};

use karyon_core::async_util::{AsyncQueue, AsyncQueueRef};
use karyon_net::{
    ByteStream, Endpoint, Result,
    transport::{Transport, TransportListener},
};

/// Bytes buffered in each direction of an in-memory pipe.
const PIPE_SIZE: usize = 64 * 1024;

/// A transport for `mem://` endpoints: each dial is an in-memory pipe
/// handed to the listener. One listener per transport.
#[derive(Clone)]
pub struct MemTransport {
    incoming: AsyncQueueRef<Box<dyn ByteStream>>,
}

impl MemTransport {
    pub fn new() -> Self {
        Self {
            incoming: AsyncQueue::new(16),
        }
    }
}

#[async_trait]
impl Transport for MemTransport {
    fn supports(&self, endpoint: &Endpoint) -> bool {
        matches!(endpoint, Endpoint::Custom { scheme, .. } if scheme == "mem")
    }

    async fn dial(&self, endpoint: &Endpoint) -> Result<Box<dyn ByteStream>> {
        let (local, remote) = duplex(PIPE_SIZE);
        let dialer = mem_endpoint("dialer");
        self.incoming
            .push(Box::new(MemStream::new(
                remote,
                dialer.clone(),
                endpoint.clone(),
            )))
            .await;
        Ok(Box::new(MemStream::new(local, endpoint.clone(), dialer)))
    }

    async fn listen(&self, endpoint: &Endpoint) -> Result<Box<dyn TransportListener>> {
        Ok(Box::new(MemListener {
            incoming: self.incoming.clone(),
            endpoint: endpoint.clone(),
        }))
    }
}

fn mem_endpoint(name: &str) -> Endpoint {
    Endpoint::Custom {
        scheme: "mem".into(),
        addr: name.into(),
    }
}

struct MemListener {
    incoming: AsyncQueueRef<Box<dyn ByteStream>>,
    endpoint: Endpoint,
}

#[async_trait]
impl TransportListener for MemListener {
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        Ok(self.incoming.recv().await)
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(self.endpoint.clone())
    }
}

/// One end of an in-memory pipe.
struct MemStream {
    inner: DuplexStream,
    peer: Endpoint,
    local: Endpoint,
}

impl MemStream {
    fn new(inner: DuplexStream, peer: Endpoint, local: Endpoint) -> Self {
        Self { inner, peer, local }
    }
}

impl ByteStream for MemStream {
    fn peer_endpoint(&self) -> Option<Endpoint> {
        Some(self.peer.clone())
    }

    fn local_endpoint(&self) -> Option<Endpoint> {
        Some(self.local.clone())
    }
}

impl AsyncRead for MemStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for MemStream {
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
