#[cfg(feature = "ws")]
use async_tungstenite::tungstenite::Message;

pub use karyon_net::codec::{ByteBuffer, Codec, Decoder, Encoder};

#[cfg(feature = "ws")]
pub use karyon_net::codec::{WebSocketCodec, WebSocketDecoder, WebSocketEncoder};

use crate::error::Error;

#[cfg(not(feature = "ws"))]
pub trait ClonableJsonCodec: Codec<Message = serde_json::Value, Error = Error> + Clone {}
#[cfg(not(feature = "ws"))]
impl<T: Codec<Message = serde_json::Value, Error = Error> + Clone> ClonableJsonCodec for T {}

#[cfg(feature = "ws")]
pub trait ClonableJsonCodec:
    Codec<Message = serde_json::Value, Error = Error>
    + WebSocketCodec<Message = serde_json::Value, Error = Error>
    + Clone
{
}
#[cfg(feature = "ws")]
impl<
        T: Codec<Message = serde_json::Value, Error = Error>
            + WebSocketCodec<Message = serde_json::Value, Error = Error>
            + Clone,
    > ClonableJsonCodec for T
{
}

#[derive(Clone)]
pub struct JsonCodec {}

impl Codec for JsonCodec {
    type Message = serde_json::Value;
    type Error = Error;
}

impl Encoder for JsonCodec {
    type EnMessage = serde_json::Value;
    type EnError = Error;
    fn encode(&self, src: &Self::EnMessage, dst: &mut ByteBuffer) -> Result<usize, Self::EnError> {
        let msg = match serde_json::to_string(src) {
            Ok(m) => m,
            Err(err) => return Err(Error::Encode(err.to_string())),
        };
        let buf = msg.as_bytes();
        dst.extend_from_slice(buf);
        Ok(buf.len())
    }
}

impl Decoder for JsonCodec {
    type DeMessage = serde_json::Value;
    type DeError = Error;
    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> Result<Option<(usize, Self::DeMessage)>, Self::DeError> {
        let de = serde_json::Deserializer::from_slice(src.as_ref());
        let mut iter = de.into_iter::<serde_json::Value>();

        let item = match iter.next() {
            Some(Ok(item)) => item,
            Some(Err(ref e)) if e.is_eof() => return Ok(None),
            Some(Err(e)) => return Err(Error::Decode(e.to_string())),
            None => return Ok(None),
        };

        Ok(Some((iter.byte_offset(), item)))
    }
}

#[cfg(feature = "ws")]
#[derive(Clone)]
pub struct WsJsonCodec {}

#[cfg(feature = "ws")]
impl WebSocketCodec for JsonCodec {
    type Message = serde_json::Value;
    type Error = Error;
}

#[cfg(feature = "ws")]
impl WebSocketEncoder for JsonCodec {
    type EnMessage = serde_json::Value;
    type EnError = Error;

    fn encode(&self, src: &Self::EnMessage) -> Result<Message, Self::EnError> {
        let msg = match serde_json::to_string(src) {
            Ok(m) => m,
            Err(err) => return Err(Error::Encode(err.to_string())),
        };
        Ok(Message::Text(msg))
    }
}

#[cfg(feature = "ws")]
impl WebSocketDecoder for JsonCodec {
    type DeMessage = serde_json::Value;
    type DeError = Error;

    fn decode(&self, src: &Message) -> Result<Option<Self::DeMessage>, Self::DeError> {
        match src {
            Message::Text(s) => match serde_json::from_str(s) {
                Ok(m) => Ok(Some(m)),
                Err(err) => Err(Error::Decode(err.to_string())),
            },
            Message::Binary(s) => match serde_json::from_slice(s) {
                Ok(m) => Ok(m),
                Err(err) => Err(Error::Decode(err.to_string())),
            },
            Message::Close(_) => Err(Error::IO(std::io::ErrorKind::ConnectionAborted.into())),
            m => Err(Error::Decode(format!(
                "Receive unexpected message: {:?}",
                m
            ))),
        }
    }
}
