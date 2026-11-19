use std::sync::Arc;

use async_trait::async_trait;

use karyon_net::Endpoint;

pub use karyon_net::ByteStream;

use crate::Result;

/// `Arc` reference to a `Transport`.
pub type TransportRef = Arc<dyn Transport>;

/// A user supplied way to dial and listen for peer connections, set with
/// `Config::transport`. The node uses it for every endpoint it
/// `supports`, and the built-in TCP, TLS and QUIC for the others.
///
/// The returned streams carry the peer protocol as is. If the transport
/// authenticates the peer with a karyon certificate, return it from
/// `ByteStream::peer_certificates` and the handshake checks the peer's id
/// against it, as with TLS.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Whether this transport handles `endpoint`.
    fn supports(&self, endpoint: &Endpoint) -> bool;

    /// Opens a connection to `endpoint`.
    async fn dial(&self, endpoint: &Endpoint) -> Result<Box<dyn ByteStream>>;

    /// Starts listening on `endpoint`.
    async fn listen(&self, endpoint: &Endpoint) -> Result<Box<dyn TransportListener>>;
}

/// Accepts the connections of a `Transport` listener.
#[async_trait]
pub trait TransportListener: Send + Sync {
    /// Waits for the next connection. Keep it cheap: it runs in the
    /// accept loop, so a slow accept holds back the next connections.
    async fn accept(&self) -> Result<Box<dyn ByteStream>>;

    /// The endpoint the listener is bound to.
    fn local_endpoint(&self) -> Result<Endpoint>;
}
