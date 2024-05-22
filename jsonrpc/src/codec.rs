#[cfg(feature = "ws")]
use async_tungstenite::tungstenite::Message;

use karyon_net::{
    codec::{Codec, Decoder, Encoder},
    Error, Result,
};

#[cfg(feature = "ws")]
use karyon_net::codec::{WebSocketCodec, WebSocketDecoder, WebSocketEncoder};

#[derive(Clone)]
pub struct JsonCodec {}

impl Codec for JsonCodec {
    type Item = serde_json::Value;
}

impl Encoder for JsonCodec {
    type EnItem = serde_json::Value;
    fn encode(&self, src: &Self::EnItem, dst: &mut [u8]) -> Result<usize> {
        let msg = match serde_json::to_string(src) {
            Ok(m) => m,
            Err(err) => return Err(Error::Encode(err.to_string())),
        };
        let buf = msg.as_bytes();
        dst[..buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }
}

impl Decoder for JsonCodec {
    type DeItem = serde_json::Value;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeItem)>> {
        let de = serde_json::Deserializer::from_slice(src);
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
impl WebSocketCodec for WsJsonCodec {
    type Item = serde_json::Value;
}

#[cfg(feature = "ws")]
impl WebSocketEncoder for WsJsonCodec {
    type EnItem = serde_json::Value;
    fn encode(&self, src: &Self::EnItem) -> Result<Message> {
        let msg = match serde_json::to_string(src) {
            Ok(m) => m,
            Err(err) => return Err(Error::Encode(err.to_string())),
        };
        Ok(Message::Text(msg))
    }
}

#[cfg(feature = "ws")]
impl WebSocketDecoder for WsJsonCodec {
    type DeItem = serde_json::Value;
    fn decode(&self, src: &Message) -> Result<Option<Self::DeItem>> {
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
