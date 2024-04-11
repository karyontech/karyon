mod bytes_codec;
mod length_codec;
mod websocket;

pub use bytes_codec::BytesCodec;
pub use length_codec::LengthCodec;
pub use websocket::{WebSocketCodec, WebSocketDecoder, WebSocketEncoder};

use crate::Result;

pub trait Codec:
    Decoder<DeItem = Self::Item> + Encoder<EnItem = Self::Item> + Send + Sync + 'static + Unpin
{
    type Item: Send + Sync;
}

pub trait Encoder {
    type EnItem;
    fn encode(&self, src: &Self::EnItem, dst: &mut [u8]) -> Result<usize>;
}

pub trait Decoder {
    type DeItem;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeItem)>>;
}
