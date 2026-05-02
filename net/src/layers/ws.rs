use std::io;

use async_tungstenite::tungstenite::Message as TungMessage;
use async_tungstenite::{WebSocketReceiver, WebSocketSender, WebSocketStream};
use futures_util::StreamExt;

#[cfg(feature = "tokio")]
use async_tungstenite::tokio::{accept_async, client_async, TokioAdapter};
#[cfg(feature = "smol")]
use async_tungstenite::{accept_async, client_async};

use crate::{
    codec::Codec,
    layer::{ClientLayer, ServerLayer},
    message::{MessageRx, MessageTx},
    ByteStream, Endpoint, Error, Result,
};

#[cfg(feature = "tokio")]
type WsInner = TokioAdapter<Box<dyn ByteStream>>;
#[cfg(feature = "smol")]
type WsInner = Box<dyn ByteStream>;

/// WebSocket message types. Wraps the underlying WS protocol
/// message kinds without exposing the third-party library.
#[derive(Debug, Clone)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

impl Message {
    /// Get the message payload as bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Message::Text(s) => s.into_bytes(),
            Message::Binary(b) => b,
            Message::Ping(b) => b,
            Message::Pong(b) => b,
            Message::Close => Vec::new(),
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Message::Text(_))
    }

    pub fn is_binary(&self) -> bool {
        matches!(self, Message::Binary(_))
    }

    pub fn is_ping(&self) -> bool {
        matches!(self, Message::Ping(_))
    }

    pub fn is_pong(&self) -> bool {
        matches!(self, Message::Pong(_))
    }

    pub fn is_close(&self) -> bool {
        matches!(self, Message::Close)
    }
}

/// WebSocket layer. Upgrades a byte stream to a WsConn.
///
/// Takes a `Codec<Message>` to encode/decode WS messages directly.
#[derive(Clone)]
pub struct WsLayer<C> {
    url: Option<String>,
    codec: C,
}

impl<C> WsLayer<C>
where
    C: Codec<Message> + Clone,
{
    /// Create a client WS layer with the target URL and codec.
    pub fn client(url: &str, codec: C) -> Self {
        Self {
            url: Some(url.to_string()),
            codec,
        }
    }

    /// Create a server WS layer with codec.
    pub fn server(codec: C) -> Self {
        Self { url: None, codec }
    }
}

impl<C> ClientLayer<Box<dyn ByteStream>, WsConn<C>> for WsLayer<C>
where
    C: Codec<Message> + Clone + Send + Sync + 'static,
    C::Message: Send + Sync + 'static,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    async fn handshake(&self, stream: Box<dyn ByteStream>) -> Result<WsConn<C>> {
        let url = self
            .url
            .as_ref()
            .ok_or_else(|| Error::MissingConfig("WS client layer requires a URL".into()))?;

        let peer = stream.peer_endpoint();
        let local = stream.local_endpoint();

        let (ws, _) = client_async(url.as_str(), stream)
            .await
            .map_err(|e| Error::IO(io::Error::other(e)))?;

        Ok(WsConn::new(ws, self.codec.clone(), peer, local))
    }
}

impl<C> ServerLayer<Box<dyn ByteStream>, WsConn<C>> for WsLayer<C>
where
    C: Codec<Message> + Clone + Send + Sync + 'static,
    C::Message: Send + Sync + 'static,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    async fn handshake(&self, stream: Box<dyn ByteStream>) -> Result<WsConn<C>> {
        let peer = stream.peer_endpoint();
        let local = stream.local_endpoint();

        let ws = accept_async(stream)
            .await
            .map_err(|e| Error::IO(io::Error::other(e)))?;

        Ok(WsConn::new(ws, self.codec.clone(), peer, local))
    }
}

// -- Convert between ws::Message and tungstenite Message --

fn from_tung(msg: TungMessage) -> Message {
    match msg {
        TungMessage::Text(t) => Message::Text(t.to_string()),
        TungMessage::Binary(b) => Message::Binary(b.to_vec()),
        TungMessage::Ping(b) => Message::Ping(b.to_vec()),
        TungMessage::Pong(b) => Message::Pong(b.to_vec()),
        TungMessage::Close(_) => Message::Close,
        _ => Message::Close,
    }
}

fn into_tung(msg: Message) -> TungMessage {
    match msg {
        Message::Text(s) => TungMessage::Text(s.into()),
        Message::Binary(b) => TungMessage::Binary(b.into()),
        Message::Ping(b) => TungMessage::Ping(b.into()),
        Message::Pong(b) => TungMessage::Pong(b.into()),
        Message::Close => TungMessage::Close(None),
    }
}

/// WebSocket message connection.
pub struct WsConn<C> {
    reader: WsReader<C>,
    writer: WsWriter<C>,
}

impl<C: Clone> WsConn<C> {
    fn new(
        ws: WebSocketStream<WsInner>,
        codec: C,
        peer_endpoint: Option<Endpoint>,
        local_endpoint: Option<Endpoint>,
    ) -> Self {
        let (sender, receiver) = ws.split();
        Self {
            reader: WsReader {
                receiver,
                codec: codec.clone(),
                peer_endpoint: peer_endpoint.clone(),
                local_endpoint: local_endpoint.clone(),
            },
            writer: WsWriter {
                sender,
                codec,
                peer_endpoint,
                local_endpoint,
            },
        }
    }
}

impl<C> WsConn<C>
where
    C: Codec<Message> + Clone + Send + Sync + 'static,
    C::Message: Send + Sync + 'static,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    /// Receive one complete message.
    pub async fn recv_msg(&mut self) -> Result<C::Message> {
        self.reader.recv_msg().await
    }

    /// Send one complete message.
    pub async fn send_msg(&mut self, msg: C::Message) -> Result<()> {
        self.writer.send_msg(msg).await
    }

    /// Remote peer address.
    pub fn peer_endpoint(&self) -> Option<Endpoint> {
        self.reader.peer_endpoint.clone()
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Option<Endpoint> {
        self.reader.local_endpoint.clone()
    }

    /// Split into independent reader and writer halves.
    pub fn split(self) -> (WsReader<C>, WsWriter<C>) {
        (self.reader, self.writer)
    }
}

/// Read half of a WebSocket connection.
pub struct WsReader<C> {
    receiver: WebSocketReceiver<WsInner>,
    codec: C,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl<C> WsReader<C>
where
    C: Codec<Message> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    /// Receive one complete message.
    pub async fn recv_msg(&mut self) -> Result<C::Message> {
        loop {
            let raw = self
                .receiver
                .next()
                .await
                .ok_or(Error::ConnectionClosed)?
                .map_err(|e| Error::IO(io::Error::other(e)))?;

            let mut ws_msg = from_tung(raw);
            match self.codec.decode(&mut ws_msg).map_err(Into::into)? {
                Some((_, item)) => return Ok(item),
                None => continue,
            }
        }
    }

    /// Remote peer address.
    pub fn peer_endpoint(&self) -> Option<Endpoint> {
        self.peer_endpoint.clone()
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Option<Endpoint> {
        self.local_endpoint.clone()
    }
}

impl<C> MessageRx for WsReader<C>
where
    C: Codec<Message> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    type Message = C::Message;

    fn recv_msg(&mut self) -> impl std::future::Future<Output = Result<Self::Message>> + Send {
        WsReader::recv_msg(self)
    }

    fn peer_endpoint(&self) -> Option<Endpoint> {
        WsReader::peer_endpoint(self)
    }
}

/// Write half of a WebSocket connection.
pub struct WsWriter<C> {
    sender: WebSocketSender<WsInner>,
    codec: C,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl<C> WsWriter<C>
where
    C: Codec<Message> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    /// Send one complete message.
    pub async fn send_msg(&mut self, msg: C::Message) -> Result<()> {
        let mut ws_msg = Message::Binary(Vec::new());
        self.codec.encode(&msg, &mut ws_msg).map_err(Into::into)?;

        self.sender
            .send(into_tung(ws_msg))
            .await
            .map_err(|e| Error::IO(io::Error::other(e)))?;
        Ok(())
    }

    /// Remote peer address.
    pub fn peer_endpoint(&self) -> Option<Endpoint> {
        self.peer_endpoint.clone()
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Option<Endpoint> {
        self.local_endpoint.clone()
    }
}

impl<C> MessageTx for WsWriter<C>
where
    C: Codec<Message> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<Error> + Send + Sync,
{
    type Message = C::Message;

    fn send_msg(
        &mut self,
        msg: Self::Message,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        WsWriter::send_msg(self, msg)
    }

    fn peer_endpoint(&self) -> Option<Endpoint> {
        WsWriter::peer_endpoint(self)
    }
}
