mod buffer;
mod bytes_codec;
mod length_codec;
#[cfg(feature = "ws")]
mod websocket;

pub use buffer::{Buffer, ByteBuffer};
pub use bytes_codec::BytesCodec;
pub use length_codec::LengthCodec;

#[cfg(feature = "ws")]
pub use websocket::{WebSocketCodec, WebSocketDecoder, WebSocketEncoder};

pub trait Codec:
    Decoder<DeMessage = Self::Message, DeError = Self::Error>
    + Encoder<EnMessage = Self::Message, EnError = Self::Error>
    + Send
    + Sync
    + Unpin
{
    type Message: Send + Sync;
    type Error;
}

pub trait Encoder {
    type EnMessage;
    type EnError: From<std::io::Error>;
    fn encode(
        &self,
        src: &Self::EnMessage,
        dst: &mut ByteBuffer,
    ) -> std::result::Result<usize, Self::EnError>;
}

pub trait Decoder {
    type DeMessage;
    type DeError: From<std::io::Error>;
    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> std::result::Result<Option<(usize, Self::DeMessage)>, Self::DeError>;
}
