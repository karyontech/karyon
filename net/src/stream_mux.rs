use std::future::Future;

use crate::{stream::ByteStream, Endpoint, Result};

/// Multiplexed connection that yields multiple independent streams
/// from a single underlying connection (e.g., QUIC).
///
/// Each stream is a `Box<dyn ByteStream>` that can be wrapped
/// with `framed_conn()` for message framing.
///
/// # Example
///
/// ```no_run
/// use karyon_net::{quic, StreamMux, Endpoint};
///
/// async {
///     let ep: Endpoint = "quic://127.0.0.1:9000".parse().unwrap();
///     // let mux = quic::QuicEndpoint::dial(&ep, config, "localhost").await?;
///     // let stream = mux.open_stream().await?;
///     // let mut conn = framed_conn(stream, codec);
/// };
/// ```
pub trait StreamMux: Send + Sync {
    /// Open a new bidirectional stream.
    fn open_stream(&self) -> impl Future<Output = Result<Box<dyn ByteStream>>> + Send;
    /// Accept a stream opened by the peer.
    fn accept_stream(&self) -> impl Future<Output = Result<Box<dyn ByteStream>>> + Send;
    /// Remote peer address.
    fn peer_endpoint(&self) -> Option<Endpoint>;
    /// Local address.
    fn local_endpoint(&self) -> Option<Endpoint>;
}
