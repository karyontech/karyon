use async_tungstenite::tungstenite::Message;

pub trait WebSocketCodec:
    WebSocketDecoder<DeMessage = Self::Message, DeError = Self::Error>
    + WebSocketEncoder<EnMessage = Self::Message, EnError = Self::Error>
    + Send
    + Sync
    + Unpin
{
    type Message: Send + Sync;
    type Error;
}

pub trait WebSocketEncoder {
    type EnMessage;
    type EnError;
    fn encode(&self, src: &Self::EnMessage) -> std::result::Result<Message, Self::EnError>;
}

pub trait WebSocketDecoder {
    type DeMessage;
    type DeError;
    fn decode(&self, src: &Message) -> std::result::Result<Option<Self::DeMessage>, Self::DeError>;
}
