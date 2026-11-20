#[cfg(any(feature = "tls", feature = "quic"))]
use std::time::Duration;

#[cfg(feature = "quic")]
pub mod quic;
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "udp")]
pub mod udp;
#[cfg(all(feature = "unix", target_family = "unix"))]
pub mod unix;

#[cfg(any(feature = "tcp", feature = "udp", feature = "quic"))]
mod resolve;

// Re-exported from `tls`.
#[cfg(feature = "tls")]
pub(crate) mod tls_listener;

/// Max time the `accept` helpers of TLS and QUIC wait for a handshake.
#[cfg(any(feature = "tls", feature = "quic"))]
const ACCEPT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
