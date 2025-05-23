use std::{
    io::ErrorKind,
    pin::Pin,
    result::Result,
    task::{Context, Poll},
};

use async_tungstenite::tungstenite::Message;
use futures_util::{
    stream::{SplitSink, SplitStream},
    Sink, SinkExt, Stream, StreamExt, TryStreamExt,
};
use pin_project_lite::pin_project;

use async_tungstenite::tungstenite::Error;

#[cfg(feature = "tokio")]
type WebSocketStream<T> =
    async_tungstenite::WebSocketStream<async_tungstenite::tokio::TokioAdapter<T>>;
#[cfg(feature = "smol")]
use async_tungstenite::WebSocketStream;

use karyon_core::async_runtime::net::TcpStream;

#[cfg(feature = "tls")]
use crate::async_rustls::TlsStream;

use crate::codec::WebSocketCodec;

pub struct WsStream<C> {
    inner: InnerWSConn,
    codec: C,
}

impl<C, E> WsStream<C>
where
    C: WebSocketCodec<Error = E> + Clone,
{
    pub fn new_ws(conn: WebSocketStream<TcpStream>, codec: C) -> Self {
        Self {
            inner: InnerWSConn::Plain(Box::new(conn)),
            codec,
        }
    }

    #[cfg(feature = "tls")]
    pub fn new_wss(conn: WebSocketStream<TlsStream<TcpStream>>, codec: C) -> Self {
        Self {
            inner: InnerWSConn::Tls(Box::new(conn)),
            codec,
        }
    }

    pub fn split(self) -> (ReadWsStream<C>, WriteWsStream<C>) {
        let (write, read) = self.inner.split();

        (
            ReadWsStream {
                codec: self.codec.clone(),
                inner: read,
            },
            WriteWsStream {
                inner: write,
                codec: self.codec,
            },
        )
    }
}

pin_project! {
    pub struct ReadWsStream<C> {
        #[pin]
        inner: SplitStream<InnerWSConn>,
        codec: C,
    }
}

pin_project! {
    pub struct WriteWsStream<C> {
        #[pin]
        inner: SplitSink<InnerWSConn, Message>,
        codec: C,
    }
}

impl<C, E> ReadWsStream<C>
where
    C: WebSocketCodec<Error = E>,
    E: From<Error>,
{
    pub async fn recv(&mut self) -> Result<C::Message, E> {
        match self.inner.next().await {
            Some(msg) => match self.codec.decode(&msg?)? {
                Some(m) => Ok(m),
                None => todo!(),
            },
            None => Err(Error::Io(std::io::Error::from(ErrorKind::ConnectionAborted)).into()),
        }
    }
}

impl<C, E> WriteWsStream<C>
where
    C: WebSocketCodec<Error = E>,
    E: From<Error>,
{
    pub async fn send(&mut self, msg: C::Message) -> Result<(), E> {
        let ws_msg = self.codec.encode(&msg)?;
        Ok(self.inner.send(ws_msg).await?)
    }
}

impl<C> Sink<Message> for WriteWsStream<C> {
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx)
    }
}

impl<C> Stream for ReadWsStream<C> {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.try_poll_next_unpin(cx)
    }
}

enum InnerWSConn {
    Plain(Box<WebSocketStream<TcpStream>>),
    #[cfg(feature = "tls")]
    Tls(Box<WebSocketStream<TlsStream<TcpStream>>>),
}

impl Sink<Message> for InnerWSConn {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s.as_mut()).poll_ready(cx),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s.as_mut()).poll_ready(cx),
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s.as_mut()).start_send(item),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s.as_mut()).start_send(item),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s.as_mut()).poll_flush(cx),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s.as_mut()).poll_close(cx),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s.as_mut()).poll_close(cx),
        }
    }
}

impl Stream for InnerWSConn {
    type Item = Result<Message, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s).poll_next(cx),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s.as_mut()).poll_next(cx),
        }
    }
}
