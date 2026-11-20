#![doc = include_str!("../README.md")]

#[cfg(all(feature = "quic", not(any(feature = "aws-lc-rs", feature = "ring"))))]
compile_error!("The `quic` feature needs a rustls crypto provider: enable `aws-lc-rs` or `ring`.");

// The OS socket transports. None on wasm, which has no OS sockets.
#[cfg(not(target_family = "wasm"))]
mod transports;

/// User supplied dial and listen (`Transport`).
pub mod transport;

mod error;

mod endpoint;

mod buffer;

pub mod codec;

mod layer;
pub mod layers;

mod message;
mod stream;
mod stream_mux;

// Framing
#[cfg(feature = "framing")]
mod framed;

#[cfg(feature = "framing")]
pub use framed::{FramedConn, FramedReader, FramedWriter, framed};

// Byte containers
pub use buffer::{Buffer, ByteBuffer, Bytes};

// Stream types
pub use stream::ByteStream;

// Message read/write traits implemented by FramedReader/FramedWriter and WsReader/WsWriter.
pub use message::{MessageRx, MessageTx};

#[cfg(feature = "ws")]
pub use layers::ws::{WsConn, WsLayer, WsLayerRef, WsReader, WsWriter};

// Layer traits
pub use layer::{ClientLayer, ServerLayer};

// Stream mux
pub use stream_mux::StreamMux;

// Endpoint types
pub use endpoint::{Addr, Endpoint, Port, ToEndpoint};

// Error types
pub use error::{Error, Result};

// Base transports: tcp, udp, unix, quic, as enabled.
#[cfg(all(
    not(target_family = "wasm"),
    any(
        feature = "tcp",
        feature = "udp",
        feature = "quic",
        all(feature = "unix", target_family = "unix")
    )
))]
pub use transports::*;

// Layers
#[cfg(feature = "proxy")]
pub use layers::proxy;

#[cfg(feature = "tls")]
pub use layers::tls;

// Re-exports
#[cfg(any(feature = "tls", feature = "quic"))]
pub use karyon_async_rustls as async_rustls;
