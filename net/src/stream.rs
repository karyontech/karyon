#[cfg(any(feature = "tls", feature = "quic"))]
use rustls_pki_types::CertificateDer;

use karyon_core::async_runtime::io::{AsyncRead, AsyncWrite};

use crate::Endpoint;

/// Byte-oriented bidirectional stream.
///
/// Implemented by TCP, TLS, Unix, and QUIC streams. Extends
/// `AsyncRead + AsyncWrite` for compatibility with IO libraries.
/// Use `framed()` to convert into a `FramedConn` with a codec.
pub trait ByteStream: AsyncRead + AsyncWrite + Send + Sync + Unpin {
    /// Remote peer address, if available.
    fn peer_endpoint(&self) -> Option<Endpoint>;
    /// Local address, if available.
    fn local_endpoint(&self) -> Option<Endpoint>;
    /// Peer certificate chain when the underlying transport authenticates
    /// the peer with X.509 (TLS, QUIC). Returns `None` for plain
    /// TCP/Unix and other unauthenticated transports.
    #[cfg(any(feature = "tls", feature = "quic"))]
    fn peer_certificates(&self) -> Option<Vec<CertificateDer<'static>>> {
        None
    }
}
