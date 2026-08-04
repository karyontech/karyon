//! Acceptors used by the stream-based and WebSocket backends.
//! Accepting is split in two phases so the accept loop never blocks on
//! a handshake: `accept` does the kernel accept, `handle` runs the
//! TLS/WS handshake and hands the split halves to the server. The loop
//! runs `handle` off to a separate task.

use std::sync::Arc;

use async_trait::async_trait;

use karyon_net::{framed, ByteStream, Endpoint};

use crate::{
    codec::JsonRpcCodec,
    error::{Error, Result},
    server::Server,
};

#[cfg(any(feature = "tls", feature = "ws"))]
use std::time::Duration;

#[cfg(any(feature = "tls", feature = "ws"))]
use karyon_core::async_util::timeout;

#[cfg(feature = "tls")]
use karyon_net::tls::TlsLayer;

#[cfg(feature = "ws")]
use std::net::SocketAddr;

#[cfg(feature = "ws")]
use karyon_net::{
    layers::ws::{WsConn, WsLayer},
    Error as NetError,
};

#[cfg(any(feature = "tls", feature = "ws"))]
use karyon_net::ServerLayer;

#[cfg(feature = "ws")]
use crate::codec::JsonRpcWsCodec;

/// Accepts raw streams and upgrades/handles them.
#[async_trait]
pub(super) trait AsyncAcceptor: Send + Sync {
    /// Kernel accept only, before any handshake.
    async fn accept(&self) -> Result<Box<dyn ByteStream>>;
    /// Upgrade the stream, frame it, and hand it to the server.
    async fn handle(&self, stream: Box<dyn ByteStream>, server: &Arc<Server>) -> Result<()>;
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

/// Byte-stream acceptor, with an optional TLS handshake.
pub(super) struct StreamAcceptor<C> {
    pub(super) listener: Box<dyn StreamListener>,
    pub(super) codec: C,
    /// Set for `tls://` endpoints; applied in `handle`.
    #[cfg(feature = "tls")]
    pub(super) tls: Option<TlsLayer>,
    #[cfg(feature = "tls")]
    pub(super) handshake_timeout: Duration,
}

#[async_trait]
impl<C> AsyncAcceptor for StreamAcceptor<C>
where
    C: JsonRpcCodec,
{
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        self.listener.accept().await.map_err(Error::from)
    }

    async fn handle(&self, stream: Box<dyn ByteStream>, server: &Arc<Server>) -> Result<()> {
        #[cfg(feature = "tls")]
        let stream = match &self.tls {
            Some(layer) => {
                timeout(
                    self.handshake_timeout,
                    ServerLayer::handshake(layer, stream),
                )
                .await??
            }
            None => stream,
        };
        let conn = framed(stream, self.codec.clone());
        let peer = conn.peer_endpoint();
        let (reader, writer) = conn.split();
        server.handle_message_conn(reader, writer, peer);
        Ok(())
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        let ep = self.listener.local_endpoint().map_err(Error::from)?;
        // The listener is plain TCP; report the TLS scheme.
        #[cfg(feature = "tls")]
        if self.tls.is_some() {
            return Ok(Endpoint::Tls(ep.addr()?, ep.port()?));
        }
        Ok(ep)
    }
}

/// WebSocket acceptor, with an optional TLS handshake for `wss://`.
#[cfg(feature = "ws")]
pub(super) struct WsAcceptor<W> {
    pub(super) listener: Box<dyn StreamListener>,
    pub(super) layer: Arc<WsLayer<W>>,
    /// Set for `wss://` endpoints; applied before the WS handshake.
    #[cfg(feature = "tls")]
    pub(super) tls: Option<TlsLayer>,
    pub(super) handshake_timeout: Duration,
}

#[cfg(feature = "ws")]
impl<W> WsAcceptor<W>
where
    W: JsonRpcWsCodec,
{
    /// Runs the TLS handshake (for `wss://`) then the WS handshake.
    async fn upgrade(&self, stream: Box<dyn ByteStream>) -> Result<WsConn<W>> {
        #[cfg(feature = "tls")]
        let stream = match &self.tls {
            Some(layer) => ServerLayer::handshake(layer, stream).await?,
            None => stream,
        };
        let conn = ServerLayer::handshake(self.layer.as_ref(), stream).await?;
        Ok(conn)
    }
}

#[cfg(feature = "ws")]
#[async_trait]
impl<W> AsyncAcceptor for WsAcceptor<W>
where
    W: JsonRpcWsCodec,
{
    async fn accept(&self) -> Result<Box<dyn ByteStream>> {
        self.listener.accept().await.map_err(Error::from)
    }

    async fn handle(&self, stream: Box<dyn ByteStream>, server: &Arc<Server>) -> Result<()> {
        // One budget for both handshakes.
        let conn = timeout(self.handshake_timeout, self.upgrade(stream)).await??;
        let peer = conn.peer_endpoint();
        let (reader, writer) = conn.split();
        server.handle_message_conn(reader, writer, peer);
        Ok(())
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        // The listener reports `tcp://...`; rewrite to the WS scheme so
        // a client building from this endpoint runs the WS handshake.
        let inner = self.listener.local_endpoint().map_err(Error::from)?;
        let addr = SocketAddr::try_from(inner.clone()).map_err(Error::from)?;
        #[cfg(feature = "tls")]
        let scheme = if self.tls.is_some() { "wss" } else { "ws" };
        #[cfg(not(feature = "tls"))]
        let scheme = "ws";
        format!("{scheme}://{addr}/")
            .parse()
            .map_err(|e: NetError| Error::from(e))
    }
}
