//! Message encoding and decoding.
//!
//! The codec trait is generic over the wire type (`W`):
//! - `ByteBuffer` for byte-stream transports (TCP, TLS, Unix)
//! - `ws::Message` for WebSocket
//!
//! Built-in codecs:
//! - `LengthCodec` - length-prefixed (u32) framing for byte streams
//! - `BytesCodec` - raw bytes with configurable max size

mod bytes_codec;
mod length_codec;

use std::result;

pub use bytes_codec::BytesCodec;
pub use length_codec::LengthCodec;

/// Encodes and decodes messages from a wire-level type.
///
/// `W` is the wire type (e.g. `ByteBuffer`, `ws::Message`).
/// `Message` is the application-level type.
pub trait Codec<W>: Send + Sync {
    type Message: Send + Sync;
    type Error;

    /// Encode a message into the wire buffer.
    fn encode(&self, src: &Self::Message, dst: &mut W) -> result::Result<usize, Self::Error>;

    /// Decode a message from the wire buffer.
    /// Returns `Some((bytes_consumed, message))` on success,
    /// `None` if more data is needed.
    fn decode(&self, src: &mut W) -> result::Result<Option<(usize, Self::Message)>, Self::Error>;
}
