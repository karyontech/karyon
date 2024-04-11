use crate::Result;
use async_tungstenite::tungstenite::Message;

pub trait WebSocketCodec:
    WebSocketDecoder<DeItem = Self::Item>
    + WebSocketEncoder<EnItem = Self::Item>
    + Send
    + Sync
    + 'static
    + Unpin
{
    type Item: Send + Sync;
}

pub trait WebSocketEncoder {
    type EnItem;
    fn encode(&self, src: &Self::EnItem) -> Result<Message>;
}

pub trait WebSocketDecoder {
    type DeItem;
    fn decode(&self, src: &Message) -> Result<Self::DeItem>;
}
