//! Acceptors used by the stream-based and WebSocket backends.
//! Accepts a connection, wraps it with a codec, and hands the split
//! halves to the server.

use std::sync::Arc;

use async_trait::async_trait;

use karyon_net::{framed, ByteStream, Endpoint};

use crate::{
    codec::JsonRpcCodec,
    error::{Error, Result},
    server::Server,
};

#[cfg(feature = "ws")]
use karyon_net::{layers::ws::WsLayer, ServerLayer};

#[cfg(feature = "ws")]
use crate::codec::JsonRpcWsCodec;

/// Produces framed connections and hands them off to the server.
#[async_trait]
pub(super) trait AsyncAcceptor: Send + Sync {
    async fn accept_and_handle(&self, server: &Arc<Server>) -> Result<()>;
    fn local_endpoint(&self) -> Result<Endpoint>;
}

/// A listener that produces byte streams.
#[async_trait]
pub(super) trait StreamListener: Send + Sync {
    async fn accept(&self) -> karyon_net::Result<Box<dyn ByteStream>>;
    fn local_endpoint(&self) -> karyon_net::Result<Endpoint>;
}

#[cfg(feature = "tcp")]
#[async_trait]
impl StreamListener for karyon_net::tcp::TcpListener {
    async fn accept(&self) -> karyon_net::Result<Box<dyn ByteStream>> {
        self.accept().await
    }
    fn local_endpoint(&self) -> karyon_net::Result<Endpoint> {
        self.local_endpoint()
    }
}

#[cfg(feature = "tls")]
#[async_trait]
impl StreamListener for karyon_net::tls::TlsListener {
    async fn accept(&self) -> karyon_net::Result<Box<dyn ByteStream>> {
        self.accept().await
    }
    fn local_endpoint(&self) -> karyon_net::Result<Endpoint> {
        self.local_endpoint()
    }
}

#[cfg(all(feature = "unix", target_family = "unix"))]
#[async_trait]
impl StreamListener for karyon_net::unix::UnixListener {
    async fn accept(&self) -> karyon_net::Result<Box<dyn ByteStream>> {
        self.accept().await
    }
    fn local_endpoint(&self) -> karyon_net::Result<Endpoint> {
        self.local_endpoint()
    }
}

/// Byte-stream acceptor: accepts a stream, wraps with a framed codec,
/// and hands the split halves to the server.
pub(super) struct StreamAcceptor<C> {
    pub(super) listener: Box<dyn StreamListener>,
    pub(super) codec: C,
}

#[async_trait]
impl<C> AsyncAcceptor for StreamAcceptor<C>
where
    C: JsonRpcCodec,
{
    async fn accept_and_handle(&self, server: &Arc<Server>) -> Result<()> {
        let stream = self.listener.accept().await?;
        let conn = framed(stream, self.codec.clone());
        let peer = conn.peer_endpoint();
        let (reader, writer) = conn.split();
        server.handle_message_conn(reader, writer, peer);
        Ok(())
    }
    fn local_endpoint(&self) -> Result<Endpoint> {
        self.listener.local_endpoint().map_err(Error::from)
    }
}

/// WebSocket acceptor: accepts a byte stream, runs the WS handshake,
/// and hands the split halves to the server.
#[cfg(feature = "ws")]
pub(super) struct WsAcceptor<W> {
    pub(super) listener: Box<dyn StreamListener>,
    pub(super) layer: Arc<WsLayer<W>>,
    /// `true` for `wss://`, used when reporting `local_endpoint`.
    pub(super) tls: bool,
}

#[cfg(feature = "ws")]
#[async_trait]
impl<W> AsyncAcceptor for WsAcceptor<W>
where
    W: JsonRpcWsCodec,
{
    async fn accept_and_handle(&self, server: &Arc<Server>) -> Result<()> {
        let stream = self.listener.accept().await?;
        let conn = ServerLayer::handshake(self.layer.as_ref(), stream).await?;
        let peer = conn.peer_endpoint();
        let (reader, writer) = conn.split();
        server.handle_message_conn(reader, writer, peer);
        Ok(())
    }
    fn local_endpoint(&self) -> Result<Endpoint> {
        // The listener reports `tcp://...`; rewrite to the WS scheme so
        // a client building from this endpoint runs the WS handshake.
        let inner = self.listener.local_endpoint().map_err(Error::from)?;
        let addr = std::net::SocketAddr::try_from(inner.clone()).map_err(Error::from)?;
        let scheme = if self.tls { "wss" } else { "ws" };
        format!("{scheme}://{addr}/")
            .parse()
            .map_err(|e: karyon_net::Error| Error::from(e))
    }
}
