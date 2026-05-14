#![doc = include_str!("../README.md")]

mod transports;

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
pub use framed::{framed, FramedConn, FramedReader, FramedWriter};

// Byte containers
pub use buffer::{Buffer, ByteBuffer, Bytes};

// Stream types
pub use stream::ByteStream;

// Message read/write traits implemented by FramedReader/FramedWriter and WsReader/WsWriter.
pub use message::{MessageRx, MessageTx};

#[cfg(feature = "ws")]
pub use layers::ws::{WsConn, WsLayer, WsReader, WsWriter};

// Layer traits
pub use layer::{ClientLayer, ServerLayer};

// Stream mux
pub use stream_mux::StreamMux;

// Endpoint types
pub use endpoint::{Addr, Endpoint, Port, ToEndpoint};

// Error types
pub use error::{Error, Result};

// Base transports
#[cfg(feature = "tcp")]
pub use transports::tcp;

#[cfg(feature = "udp")]
pub use transports::udp;

#[cfg(all(feature = "unix", target_family = "unix"))]
pub use transports::unix;

#[cfg(feature = "quic")]
pub use transports::quic;

// Layers
#[cfg(feature = "proxy")]
pub use layers::proxy;

#[cfg(feature = "tls")]
pub use layers::tls;

// Re-exports
#[cfg(any(feature = "tls", feature = "quic"))]
pub use karyon_async_rustls as async_rustls;
