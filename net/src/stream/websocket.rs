use std::{
    pin::Pin,
    task::{Context, Poll},
};

use async_tungstenite::tungstenite::Message;
use futures_util::{
    stream::{SplitSink, SplitStream},
    Sink, SinkExt, Stream, StreamExt, TryStreamExt,
};
use pin_project_lite::pin_project;

#[cfg(all(feature = "smol", feature = "tls"))]
use futures_rustls::TlsStream;
#[cfg(all(feature = "tokio", feature = "tls"))]
use tokio_rustls::TlsStream;

use karyon_core::async_runtime::net::TcpStream;

use crate::{codec::WebSocketCodec, Error, Result};

#[cfg(feature = "tokio")]
type WebSocketStream<T> =
    async_tungstenite::WebSocketStream<async_tungstenite::tokio::TokioAdapter<T>>;
#[cfg(feature = "smol")]
use async_tungstenite::WebSocketStream;

pub struct WsStream<C> {
    inner: InnerWSConn,
    codec: C,
}

impl<C> WsStream<C>
where
    C: WebSocketCodec + Clone,
{
    pub fn new_ws(conn: WebSocketStream<TcpStream>, codec: C) -> Self {
        Self {
            inner: InnerWSConn::Plain(conn),
            codec,
        }
    }

    #[cfg(feature = "tls")]
    pub fn new_wss(conn: WebSocketStream<TlsStream<TcpStream>>, codec: C) -> Self {
        Self {
            inner: InnerWSConn::Tls(conn),
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

impl<C> ReadWsStream<C>
where
    C: WebSocketCodec,
{
    pub async fn recv(&mut self) -> Result<C::Item> {
        match self.inner.next().await {
            Some(msg) => match self.codec.decode(&msg?)? {
                Some(m) => Ok(m),
                None => todo!(),
            },
            None => Err(Error::IO(std::io::ErrorKind::ConnectionAborted.into())),
        }
    }
}

impl<C> WriteWsStream<C>
where
    C: WebSocketCodec,
{
    pub async fn send(&mut self, msg: C::Item) -> Result<()> {
        let ws_msg = self.codec.encode(&msg)?;
        self.inner.send(ws_msg).await
    }
}

impl<C> Sink<Message> for WriteWsStream<C> {
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Message) -> Result<()> {
        self.project().inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.project().inner.poll_close(cx)
    }
}

impl<C> Stream for ReadWsStream<C> {
    type Item = Result<Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.try_poll_next_unpin(cx)
    }
}

enum InnerWSConn {
    Plain(WebSocketStream<TcpStream>),
    #[cfg(feature = "tls")]
    Tls(WebSocketStream<TlsStream<TcpStream>>),
}

impl Sink<Message> for InnerWSConn {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s).poll_ready(cx).map_err(Error::from),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s).poll_ready(cx).map_err(Error::from),
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<()> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s).start_send(item).map_err(Error::from),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s).start_send(item).map_err(Error::from),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s).poll_flush(cx).map_err(Error::from),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s).poll_flush(cx).map_err(Error::from),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s).poll_close(cx).map_err(Error::from),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s).poll_close(cx).map_err(Error::from),
        }
        .map_err(Error::from)
    }
}

impl Stream for InnerWSConn {
    type Item = Result<Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut *self {
            InnerWSConn::Plain(s) => Pin::new(s).poll_next(cx).map_err(Error::from),
            #[cfg(feature = "tls")]
            InnerWSConn::Tls(s) => Pin::new(s).poll_next(cx).map_err(Error::from),
        }
    }
}
