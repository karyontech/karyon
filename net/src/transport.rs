use std::sync::Arc;

use async_trait::async_trait;

use crate::{ByteStream, Endpoint, Result};

/// `Arc` reference to a `Transport`.
pub type TransportRef = Arc<dyn Transport>;

/// A user supplied way to dial and listen, e.g. in-memory pipes, sockets
/// from another runtime, or a WASI host's sockets. Crates on karyon_net use
/// it for every endpoint it `supports`, and their built-in transports for
/// the others. Pair it with a custom endpoint scheme (`mem://node-a`) or
/// take over a built-in one (`tcp://`, `tls://`, `quic://`).
///
/// The returned streams are used as is: no TLS or other layer is added.
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
