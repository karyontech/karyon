//! JSON codec for JSON-RPC message framing.
//!
//! Provides `JsonCodec` which encodes/decodes `serde_json::Value`
//! messages with a configurable max payload size (default 4MB).

use std::io;

pub use karyon_net::codec::Codec;
pub use karyon_net::ByteBuffer;

#[cfg(feature = "ws")]
use karyon_net::layers::ws::Message as WsMessage;

const DEFAULT_MAX_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4MB

/// Byte-stream codec used for TCP, TLS, Unix, QUIC, and HTTP.
pub trait JsonRpcCodec:
    Codec<ByteBuffer, Message = serde_json::Value, Error = karyon_net::Error>
    + Clone
    + Send
    + Sync
    + 'static
{
}

impl<T> JsonRpcCodec for T where
    T: Codec<ByteBuffer, Message = serde_json::Value, Error = karyon_net::Error>
        + Clone
        + Send
        + Sync
        + 'static
{
}

/// WebSocket-message codec used for `ws://` and `wss://` endpoints.
#[cfg(feature = "ws")]
pub trait JsonRpcWsCodec:
    Codec<WsMessage, Message = serde_json::Value, Error = karyon_net::Error>
    + Clone
    + Send
    + Sync
    + 'static
{
}

#[cfg(feature = "ws")]
impl<T> JsonRpcWsCodec for T where
    T: Codec<WsMessage, Message = serde_json::Value, Error = karyon_net::Error>
        + Clone
        + Send
        + Sync
        + 'static
{
}

/// Default JSON codec with configurable max payload size.
#[derive(Clone)]
pub struct JsonCodec {
    max_size: usize,
}

impl Default for JsonCodec {
    fn default() -> Self {
        Self {
            max_size: DEFAULT_MAX_BUFFER_SIZE,
        }
    }
}

impl JsonCodec {
    pub fn new(max_payload_size: usize) -> Self {
        Self {
            max_size: max_payload_size,
        }
    }
}

impl Codec<ByteBuffer> for JsonCodec {
    type Message = serde_json::Value;
    type Error = karyon_net::Error;

    fn encode(
        &self,
        src: &serde_json::Value,
        dst: &mut ByteBuffer,
    ) -> std::result::Result<usize, karyon_net::Error> {
        let msg =
            serde_json::to_string(src).map_err(|e| karyon_net::Error::IO(io::Error::other(e)))?;
        let buf = msg.as_bytes();

        if buf.len() > self.max_size {
            return Err(karyon_net::Error::BufferFull(format!(
                "Buffer size {} exceeds maximum {}",
                buf.len(),
                self.max_size
            )));
        }

        dst.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> std::result::Result<Option<(usize, serde_json::Value)>, karyon_net::Error> {
        if src.len() > self.max_size {
            return Err(karyon_net::Error::BufferFull(format!(
                "Buffer size {} exceeds maximum {}",
                src.len(),
                self.max_size
            )));
        }

        let de = serde_json::Deserializer::from_slice(src.as_ref());
        let mut iter = de.into_iter::<serde_json::Value>();

        let item = match iter.next() {
            Some(Ok(item)) => item,
            Some(Err(ref e)) if e.is_eof() => return Ok(None),
            Some(Err(e)) => return Err(karyon_net::Error::IO(io::Error::other(e))),
            None => return Ok(None),
        };

        Ok(Some((iter.byte_offset(), item)))
    }
}

/// WS codec for JSON values over WebSocket text messages.
#[cfg(feature = "ws")]
impl Codec<WsMessage> for JsonCodec {
    type Message = serde_json::Value;
    type Error = karyon_net::Error;

    fn encode(
        &self,
        src: &serde_json::Value,
        dst: &mut WsMessage,
    ) -> std::result::Result<usize, karyon_net::Error> {
        let json =
            serde_json::to_string(src).map_err(|e| karyon_net::Error::IO(io::Error::other(e)))?;
        let len = json.len();
        *dst = WsMessage::Text(json);
        Ok(len)
    }

    fn decode(
        &self,
        src: &mut WsMessage,
    ) -> std::result::Result<Option<(usize, serde_json::Value)>, karyon_net::Error> {
        match src {
            WsMessage::Text(text) => {
                let len = text.len();
                if len > self.max_size {
                    return Err(karyon_net::Error::BufferFull(format!(
                        "Buffer size {} exceeds maximum {}",
                        len, self.max_size
                    )));
                }
                let val = serde_json::from_str(text)
                    .map_err(|e| karyon_net::Error::IO(io::Error::other(e)))?;
                Ok(Some((len, val)))
            }
            _ => Ok(None),
        }
    }
}
